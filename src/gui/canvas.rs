//! A tile of the window's picture (0x00RRGGBB per pixel). The program never
//! keeps a framebuffer of the whole window: it draws one rectangle (a
//! panel, or a strip of a full redraw) at a time straight into the
//! MIT-SHM segment and sends it, so the memory is that of the largest tile
//! (1 MiB), whatever the window size. Every operation takes window
//! coordinates and is clipped to the tile.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        let i = self.intersect(o);
        i.w > 0 && i.h > 0
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

enum Buf {
    Owned(Vec<u32>),
    /// Memory owned by someone else (the shared memory segment).
    Raw(*mut u32, usize),
}

pub struct Canvas {
    /// The tile in window coordinates.
    pub tile: Rect,
    buf: Buf,
    clip: Rect,
}

impl Canvas {
    /// A canvas with its own memory, `w` x `h` at the origin.
    pub fn new(w: i32, h: i32) -> Self {
        let (w, h) = (w.max(1), h.max(1));
        let r = Rect::new(0, 0, w, h);
        Self {
            tile: r,
            buf: Buf::Owned(vec![0; (w * h) as usize]),
            clip: r,
        }
    }

    /// A canvas of up to `cap` pixels in its own memory.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            tile: Rect::new(0, 0, 1, 1),
            buf: Buf::Owned(vec![0; cap.max(1)]),
            clip: Rect::new(0, 0, 1, 1),
        }
    }

    /// A canvas drawing into `len` pixels at `ptr`.
    ///
    /// # Safety
    /// The memory must stay valid and unaliased while the canvas lives.
    pub unsafe fn from_raw(ptr: *mut u32, len: usize) -> Self {
        Self {
            tile: Rect::new(0, 0, 1, 1),
            buf: Buf::Raw(ptr, len),
            clip: Rect::new(0, 0, 1, 1),
        }
    }

    pub fn capacity(&self) -> usize {
        match &self.buf {
            Buf::Owned(v) => v.len(),
            Buf::Raw(_, n) => *n,
        }
    }

    /// Point the canvas at another tile (its pixels are undefined until
    /// drawn).
    pub fn set_tile(&mut self, r: Rect) {
        debug_assert!((r.w * r.h) as usize <= self.capacity());
        self.tile = r;
        self.clip = r;
    }

    pub fn px(&mut self) -> &mut [u32] {
        let n = (self.tile.w * self.tile.h).max(0) as usize;
        match &mut self.buf {
            Buf::Owned(v) => &mut v[..n],
            Buf::Raw(p, len) => unsafe { std::slice::from_raw_parts_mut(*p, n.min(*len)) },
        }
    }

    pub fn pixels(&self) -> &[u32] {
        let n = (self.tile.w * self.tile.h).max(0) as usize;
        match &self.buf {
            Buf::Owned(v) => &v[..n],
            Buf::Raw(p, len) => unsafe { std::slice::from_raw_parts(*p, n.min(*len)) },
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        let p = self.pixels();
        // SAFETY: u32 has no padding; the slice covers exactly the pixels.
        unsafe { std::slice::from_raw_parts(p.as_ptr() as *const u8, p.len() * 4) }
    }

    /// The pixel at window coordinates (for tests).
    pub fn get(&self, x: i32, y: i32) -> u32 {
        let t = self.tile;
        if !t.contains(x, y) {
            return 0;
        }
        self.pixels()[((y - t.y) * t.w + (x - t.x)) as usize]
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&self.tile);
    }
    pub fn clip(&self) -> Rect {
        self.clip
    }
    pub fn unclip(&mut self) {
        self.clip = self.tile;
    }
    /// True when `r` touches what this tile draws.
    pub fn sees(&self, r: &Rect) -> bool {
        self.clip.overlaps(r)
    }

    pub fn fill(&mut self, r: Rect, color: u32) {
        let r = r.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        let t = self.tile;
        let px = self.px();
        for y in r.y..r.y + r.h {
            let row = ((y - t.y) * t.w + (r.x - t.x)) as usize;
            px[row..row + r.w as usize].fill(color);
        }
    }

    /// A frame `width` pixels wide inside `r`.
    pub fn frame(&mut self, r: Rect, color: u32, width: i32) {
        let t = width.max(1);
        self.fill(Rect::new(r.x, r.y, r.w, t), color);
        self.fill(Rect::new(r.x, r.y + r.h - t, r.w, t), color);
        self.fill(Rect::new(r.x, r.y, t, r.h), color);
        self.fill(Rect::new(r.x + r.w - t, r.y, t, r.h), color);
    }

    #[inline]
    fn mix(d: u32, color: u32, a: u32) -> u32 {
        let inv = 255 - a;
        let ch = |s: u32, d: u32| (s * a + d * inv + 127) / 255;
        (ch((color >> 16) & 0xff, (d >> 16) & 0xff) << 16)
            | (ch((color >> 8) & 0xff, (d >> 8) & 0xff) << 8)
            | ch(color & 0xff, d & 0xff)
    }

    /// Blend a translucent rectangle (glow, scanlines, hover).
    pub fn tint(&mut self, r: Rect, color: u32, a: u32) {
        if a == 0 {
            return;
        }
        if a >= 255 {
            return self.fill(r, color);
        }
        let r = r.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        let t = self.tile;
        let px = self.px();
        for y in r.y..r.y + r.h {
            let row = ((y - t.y) * t.w + (r.x - t.x)) as usize;
            for p in &mut px[row..row + r.w as usize] {
                *p = Self::mix(*p, color, a);
            }
        }
    }

    /// A one-pixel column from `y0` (top) to `y1` (exclusive) blended from
    /// alpha `a0` at the top to `a1` at the bottom (fixed point, no division
    /// per pixel).
    pub fn vgrad(&mut self, x: i32, y0: i32, y1: i32, color: u32, a0: u32, a1: u32) {
        let c = self.clip;
        if x < c.x || x >= c.right() || y1 <= y0 {
            return;
        }
        let (ys, ye) = (y0.max(c.y), y1.min(c.bottom()));
        if ys >= ye {
            return;
        }
        let t = self.tile;
        let span = (y1 - y0).max(1);
        let step = ((a1 as i32 - a0 as i32) << 16) / span;
        let mut a = ((a0 as i32) << 16) + step * (ys - y0);
        let (cr, cg, cb) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
        let w = t.w as usize;
        let mut i = ((ys - t.y) * t.w + (x - t.x)) as usize;
        let px = self.px();
        for _ in ys..ye {
            let al = (a >> 16).clamp(0, 255) as u32;
            a += step;
            if al >= 255 {
                px[i] = color;
            } else if al != 0 {
                let d = px[i];
                let inv = 255 - al;
                let r = (cr * al + ((d >> 16) & 0xff) * inv + 128) >> 8;
                let g = (cg * al + ((d >> 8) & 0xff) * inv + 128) >> 8;
                let b = (cb * al + (d & 0xff) * inv + 128) >> 8;
                px[i] = (r << 16) | (g << 8) | b;
            }
            i += w;
        }
    }

    /// Darken every other row of `r` by `k`/256 (CRT scanlines): a shift
    /// and a mask per pixel.
    pub fn scanlines(&mut self, r: Rect, k: u32) {
        let r = r.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        let t = self.tile;
        let keep = 256 - k.min(256);
        let px = self.px();
        let mut y = r.y + ((r.y + 1) & 1);
        while y < r.bottom() {
            let row = ((y - t.y) * t.w + (r.x - t.x)) as usize;
            for p in &mut px[row..row + r.w as usize] {
                let d = *p;
                let rb = (((d & 0xff00ff) * keep) >> 8) & 0xff00ff;
                let g = (((d & 0x00ff00) * keep) >> 8) & 0x00ff00;
                *p = rb | g;
            }
            y += 2;
        }
    }

    /// Copy `src` (an `r.w` x `r.h` picture of window rectangle `r`) into
    /// the tile, clipped.
    pub fn blit(&mut self, r: Rect, src: &[u32]) {
        let a = r.intersect(&self.clip);
        if a.is_empty() || src.len() < (r.w * r.h) as usize {
            return;
        }
        let t = self.tile;
        let px = self.px();
        for y in a.y..a.bottom() {
            let s0 = ((y - r.y) * r.w + (a.x - r.x)) as usize;
            let d0 = ((y - t.y) * t.w + (a.x - t.x)) as usize;
            px[d0..d0 + a.w as usize].copy_from_slice(&src[s0..s0 + a.w as usize]);
        }
    }

    /// Blend `src` (ARGB, alpha in the top byte, `r.w` x `r.h`) over the
    /// tile, clipped.
    pub fn blend_argb(&mut self, r: Rect, src: &[u32]) {
        let a = r.intersect(&self.clip);
        if a.is_empty() || src.len() < (r.w * r.h) as usize {
            return;
        }
        let t = self.tile;
        let px = self.px();
        for y in a.y..a.bottom() {
            for x in a.x..a.right() {
                let s = src[((y - r.y) * r.w + (x - r.x)) as usize];
                let al = s >> 24;
                if al == 0 {
                    continue;
                }
                let d = &mut px[((y - t.y) * t.w + (x - t.x)) as usize];
                *d = Self::mix(*d, s & 0xff_ffff, al);
            }
        }
    }

    /// Move the whole tile's picture `k` pixels to the left (the rightmost
    /// `k` columns keep their old pixels).
    pub fn shift_left(&mut self, k: i32) {
        let t = self.tile;
        if k <= 0 || k >= t.w {
            return;
        }
        let w = t.w as usize;
        let k = k as usize;
        let px = self.px();
        for row in px.chunks_exact_mut(w) {
            row.copy_within(k.., 0);
        }
    }

    /// Draw a coverage mask (one byte per pixel) in `color` at (x, y).
    pub fn mask(&mut self, x: i32, y: i32, w: i32, h: i32, coverage: &[u8], color: u32) {
        let area = Rect::new(x, y, w, h).intersect(&self.clip);
        if area.is_empty() {
            return;
        }
        let t = self.tile;
        let px = self.px();
        for py in area.y..area.y + area.h {
            let src_row = ((py - y) * w) as usize;
            let dst_row = ((py - t.y) * t.w) as usize;
            for qx in area.x..area.x + area.w {
                let a = coverage[src_row + (qx - x) as usize] as u32;
                if a == 0 {
                    continue;
                }
                let i = dst_row + (qx - t.x) as usize;
                px[i] = if a >= 255 {
                    color
                } else {
                    Self::mix(px[i], color, a)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_use_window_coordinates() {
        let mut c = Canvas::with_capacity(100);
        c.set_tile(Rect::new(50, 20, 10, 10));
        c.fill(Rect::new(0, 0, 1000, 1000), 0x111111);
        c.fill(Rect::new(55, 25, 100, 100), 0xffffff);
        assert_eq!(c.get(54, 24), 0x111111);
        assert_eq!(c.get(55, 25), 0xffffff);
        assert_eq!(c.get(59, 29), 0xffffff);
        // Outside the tile: nothing drawn, nothing panics.
        c.fill(Rect::new(0, 0, 10, 10), 0x222222);
        c.set_clip(Rect::new(50, 20, 2, 2));
        c.fill(Rect::new(0, 0, 1000, 1000), 0x333333);
        assert_eq!(c.get(51, 21), 0x333333);
        assert_eq!(c.get(52, 22), 0x111111);
    }

    #[test]
    fn gradients_scanlines_blit_and_shift() {
        let mut c = Canvas::new(4, 10);
        c.vgrad(1, 0, 10, 0xffffff, 255, 0);
        assert_eq!(c.get(1, 0), 0xffffff);
        assert!(c.get(1, 9) & 0xff < 40);
        assert_eq!(c.get(0, 0), 0);
        c.fill(Rect::new(0, 0, 4, 10), 0x808080);
        c.scanlines(Rect::new(0, 0, 4, 10), 128);
        assert_eq!(c.get(0, 0), 0x808080);
        assert_eq!(c.get(0, 1), 0x404040);
        let src: Vec<u32> = (0..4).collect();
        c.blit(Rect::new(0, 0, 4, 1), &src);
        assert_eq!(c.get(3, 0), 3);
        c.shift_left(1);
        assert_eq!(c.get(0, 0), 1);
        assert_eq!(c.get(2, 0), 3);
    }

    #[test]
    fn half_coverage_blends_halfway() {
        let mut c = Canvas::new(1, 1);
        c.mask(0, 0, 1, 1, &[128], 0xffffff);
        let v = c.get(0, 0) & 0xff;
        assert!((126..=130).contains(&v), "{v}");
        c.tint(Rect::new(0, 0, 1, 1), 0x000000, 255);
        assert_eq!(c.get(0, 0), 0);
    }
}
