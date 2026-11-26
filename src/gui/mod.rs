//! The NIRU X11 toolkit as NIRUSETTINGS uses it, copied from NIRUSYS: a
//! tile canvas in shared memory, text in the theme font and the 8x8 pixel
//! font from NIRUSID, the live NIRUX theme, and the event loop. The modules
//! are shared by copy with the other NIRU tools, so not every function is
//! used.
#![allow(dead_code)]
pub mod canvas;
pub mod pixfont;
pub mod shell;
pub mod text;
pub mod theme;
pub mod x;

use canvas::{Canvas, Rect};
use theme::{blend, Palette};

/// ARGB window icons: a control panel with three sliders.
pub fn icons(p: &Palette) -> Vec<(u32, u32, Vec<u32>)> {
    [16u32, 32, 48]
        .iter()
        .map(|&s| {
            let si = s as i32;
            let mut cv = Canvas::new(si, si);
            let b = (si / 10).max(1);
            cv.fill(Rect::new(0, 0, si, si), blend(p.border, p.accent, 0.5));
            cv.fill(
                Rect::new(b, b, si - 2 * b, si - 2 * b),
                blend(p.background, 0, 0.3),
            );
            let inner = si - 2 * b;
            for (i, pos) in [0.3f32, 0.7, 0.45].iter().enumerate() {
                let y = b + inner * (i as i32 + 1) / 4;
                let t = (si / 16).max(1);
                cv.fill(Rect::new(b + 2, y, inner - 4, t), p.muted);
                let kx = b + 2 + ((inner - 4) as f32 * pos) as i32;
                let k = (si / 8).max(2);
                cv.fill(Rect::new(kx - k / 2, y - k / 2, k, k + t), p.accent);
            }
            let argb = cv.pixels().iter().map(|v| 0xff00_0000 | v).collect();
            (s, s, argb)
        })
        .collect()
}
