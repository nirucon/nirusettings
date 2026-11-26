//! Text with the suite's monospace font. Glyph coverage masks are cached per
//! (character, size), so each distinct character is rasterised once. The
//! font file is mapped, not read: only the pages of the glyphs in use ever
//! become resident (a Nerd Font is several megabytes).
use super::canvas::Canvas;
use ab_glyph::{point, Font, FontArc, FontRef, PxScale, ScaleFont};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A small multiplicative hasher: the keys are (char, size) pairs and
/// SipHash would cost more than the lookups themselves.
#[derive(Default)]
pub struct FastHash(u64);

impl Hasher for FastHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(*b as u64);
        }
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type Map<K, V> = HashMap<K, V, BuildHasherDefault<FastHash>>;

struct Mask {
    w: i32,
    h: i32,
    x: i32,
    y: i32,
    coverage: Vec<u8>,
}

/// Direct tables for the ASCII characters of one size (no hashing).
struct Ascii {
    px: u32,
    adv: [f32; 128],
    masks: Vec<Option<Option<Mask>>>,
}

pub struct Text {
    ascii: Vec<Ascii>,
    font: FontArc,
    fallback: Option<Option<FontArc>>,
    masks: Map<(char, u32), Option<Mask>>,
    advances: Map<(char, u32), f32>,
}

/// One size of the font: metrics in whole pixels.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub px: f32,
    pub ascent: i32,
    pub height: i32,
}

impl Text {
    pub fn new(font_path: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            font: load_font(font_path, MONO)?,
            fallback: None,
            ascii: Vec::new(),
            masks: Map::default(),
            advances: Map::default(),
        })
    }

    pub fn size(&self, px: f32) -> Size {
        let s = self.font.as_scaled(PxScale::from(px));
        Size {
            px,
            ascent: s.ascent().ceil() as i32,
            height: s.height().ceil() as i32,
        }
    }

    fn font_for(&mut self, c: char) -> FontArc {
        if self.font.glyph_id(c).0 != 0 || c.is_ascii() {
            return self.font.clone();
        }
        let fb = self
            .fallback
            .get_or_insert_with(|| load_font(None, SYMBOLS).ok())
            .clone();
        match fb {
            Some(f) if f.glyph_id(c).0 != 0 => f,
            _ => self.font.clone(),
        }
    }

    fn slot(&mut self, size: Size) -> usize {
        let bits = size.px.to_bits();
        if let Some(i) = self.ascii.iter().position(|a| a.px == bits) {
            return i;
        }
        let mut a = Ascii {
            px: bits,
            adv: [0.0; 128],
            masks: (0..128).map(|_| None).collect(),
        };
        let s = self.font.as_scaled(PxScale::from(size.px));
        for (i, v) in a.adv.iter_mut().enumerate() {
            *v = s.h_advance(self.font.glyph_id(i as u8 as char));
        }
        self.ascii.push(a);
        self.ascii.len() - 1
    }

    pub fn advance(&mut self, c: char, size: Size) -> f32 {
        if (c as u32) < 128 {
            let i = self.slot(size);
            return self.ascii[i].adv[c as usize];
        }
        let key = (c, size.px.to_bits());
        if let Some(a) = self.advances.get(&key) {
            return *a;
        }
        let f = self.font_for(c);
        let s = f.as_scaled(PxScale::from(size.px));
        let a = s.h_advance(f.glyph_id(c));
        self.advances.insert(key, a);
        a
    }

    pub fn width(&mut self, s: &str, size: Size) -> i32 {
        let slot = self.slot(size);
        let mut w = 0.0f32;
        for c in s.chars() {
            w += if (c as u32) < 128 {
                self.ascii[slot].adv[c as usize]
            } else {
                self.advance(c, size)
            };
        }
        w.ceil() as i32
    }

    /// The longest prefix of `s` that fits `max` pixels, with "…" when cut.
    pub fn ellipsize(&mut self, s: &str, size: Size, max: i32) -> String {
        if self.width(s, size) <= max {
            return s.to_string();
        }
        let dots = self.advance('…', size);
        let mut out = String::new();
        let mut w = 0.0;
        for c in s.chars() {
            let a = self.advance(c, size);
            if (w + a + dots).ceil() as i32 > max {
                break;
            }
            w += a;
            out.push(c);
        }
        out.push('…');
        out
    }

    /// Break running text into lines of at most `max` pixels: at spaces
    /// when possible, inside a word only when the word alone is too wide.
    /// A newline always starts a new line.
    pub fn wrap(&mut self, s: &str, size: Size, max: i32) -> Vec<String> {
        let mut out = Vec::new();
        for para in s.split('\n') {
            let mut line = String::new();
            let mut w = 0.0f32;
            for word in para.split_inclusive(' ') {
                let ww: f32 = word.chars().map(|c| self.advance(c, size)).sum();
                let trimmed: f32 = word
                    .trim_end_matches(' ')
                    .chars()
                    .map(|c| self.advance(c, size))
                    .sum();
                if (w + trimmed).ceil() as i32 <= max
                    || line.is_empty() && trimmed.ceil() as i32 <= max
                {
                    line.push_str(word);
                    w += ww;
                    continue;
                }
                if !line.is_empty() {
                    out.push(line.trim_end().to_string());
                    line = String::new();
                    w = 0.0;
                }
                if trimmed.ceil() as i32 <= max {
                    line.push_str(word);
                    w = ww;
                    continue;
                }
                // A word longer than the line: cut it by characters.
                for c in word.chars() {
                    let a = self.advance(c, size);
                    if (w + a).ceil() as i32 > max && !line.is_empty() {
                        out.push(std::mem::take(&mut line));
                        w = 0.0;
                    }
                    line.push(c);
                    w += a;
                }
            }
            out.push(line.trim_end().to_string());
        }
        out
    }

    /// Draw `s` with its baseline at `y + size.ascent`. Returns the width.
    pub fn draw(
        &mut self,
        cv: &mut Canvas,
        x: i32,
        y: i32,
        s: &str,
        size: Size,
        color: u32,
    ) -> i32 {
        let mut pen = x as f32;
        let base = y + size.ascent;
        let slot = self.slot(size);
        for c in s.chars() {
            if (c as u32) < 128 {
                let ci = c as usize;
                if self.ascii[slot].masks[ci].is_none() {
                    let m = self.rasterise(c, size.px);
                    self.ascii[slot].masks[ci] = Some(m);
                }
                let a = &self.ascii[slot];
                if let Some(Some(m)) = &a.masks[ci] {
                    cv.mask(
                        pen.round() as i32 + m.x,
                        base + m.y,
                        m.w,
                        m.h,
                        &m.coverage,
                        color,
                    );
                }
                pen += a.adv[ci];
                continue;
            }
            let key = (c, size.px.to_bits());
            if !self.masks.contains_key(&key) {
                let m = self.rasterise(c, size.px);
                self.masks.insert(key, m);
            }
            if let Some(Some(m)) = self.masks.get(&key) {
                cv.mask(
                    pen.round() as i32 + m.x,
                    base + m.y,
                    m.w,
                    m.h,
                    &m.coverage,
                    color,
                );
            }
            pen += self.advance(c, size);
        }
        (pen - x as f32).ceil() as i32
    }

    fn rasterise(&mut self, c: char, px: f32) -> Option<Mask> {
        if c == ' ' {
            return None;
        }
        let f = self.font_for(c);
        let glyph = f
            .glyph_id(c)
            .with_scale_and_position(PxScale::from(px), point(0.0, 0.0));
        let outline = f.outline_glyph(glyph)?;
        let b = outline.px_bounds();
        let (w, h) = (b.width().ceil() as i32, b.height().ceil() as i32);
        if w <= 0 || h <= 0 {
            return None;
        }
        let mut coverage = vec![0u8; (w * h) as usize];
        outline.draw(|gx, gy, v| {
            let i = (gy as i32 * w + gx as i32) as usize;
            if i < coverage.len() {
                coverage[i] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        });
        Some(Mask {
            w,
            h,
            x: b.min.x.floor() as i32,
            y: b.min.y.floor() as i32,
            coverage,
        })
    }
}

const MONO: &str = "JetBrainsMono Nerd Font,JetBrains Mono,DejaVu Sans Mono:style=Regular";
const SYMBOLS: &str = "DejaVu Sans,Noto Sans Symbols2,Noto Sans";

/// Map a font file read-only for the life of the program.
fn map_font(p: &str) -> Result<FontArc, String> {
    use std::os::fd::AsRawFd;
    let f = std::fs::File::open(p).map_err(|e| format!("font {p}: {e}"))?;
    let len = f.metadata().map_err(|e| format!("font {p}: {e}"))?.len() as usize;
    if len == 0 {
        return Err(format!("font {p}: empty"));
    }
    let addr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            f.as_raw_fd(),
            0,
        )
    };
    if addr == libc::MAP_FAILED {
        return Err(format!("font {p}: cannot map"));
    }
    // SAFETY: the mapping is never unmapped (a theme font change maps the
    // new file and leaves the old pages for the kernel to drop).
    let bytes: &'static [u8] = unsafe { std::slice::from_raw_parts(addr as *const u8, len) };
    let font = FontRef::try_from_slice(bytes).map_err(|e| format!("font {p}: {e}"))?;
    Ok(FontArc::new(font))
}

fn load_font(path: Option<&str>, pattern: &str) -> Result<FontArc, String> {
    if let Some(p) = path {
        return map_font(p);
    }
    let mut tried = Vec::new();
    // fc-match costs a few milliseconds at every start; its answer is kept
    // in the cache and used while the file exists.
    let cache = nirusettings::paths::cache_dir().join(format!(
        "font-{}.txt",
        pattern
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
    ));
    if let Ok(p) = std::fs::read_to_string(&cache) {
        if let Ok(f) = map_font(p.trim()) {
            return Ok(f);
        }
    }
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            if let Ok(f) = map_font(&p) {
                let _ = nirusettings::paths::write_atomic(&cache, p.as_bytes());
                return Ok(f);
            }
            tried.push(p);
        }
    }
    for p in [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    ] {
        if let Ok(f) = map_font(p) {
            return Ok(f);
        }
        tried.push(p.to_string());
    }
    Err(format!("no usable font (tried {})", tried.join(", ")))
}
