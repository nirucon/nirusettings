//! Drawing the window: a retro control panel in the NIRUX theme colours.
//! A header with the pixel-font name and a CRT rule, the sections on the
//! left, the settings in a framed panel, help and status at the bottom,
//! faint scanlines over everything.
use crate::app::{Colors, Focus, Row, SettingsApp, Target, Tone};
use crate::gui::canvas::{Canvas, Rect};
use crate::gui::pixfont;
use crate::gui::shell::Frame;
use crate::gui::text::{Size, Text};
use crate::gui::theme::{blend, luminance, Palette};
use nirusettings::schema::{self, Kind, SECTIONS};
use nirusettings::store::{self, Source};
use toml::Value;

/// `c`, moved toward `toward` until it stands out from `bg`.
fn contrast(c: u32, bg: u32, toward: u32, min: f32) -> u32 {
    let mut out = c;
    let mut t = 0.0;
    while (luminance(out) - luminance(bg)).abs() < min && t < 0.8 {
        t += 0.1;
        out = blend(c, toward, t);
    }
    out
}

pub fn colors(p: &Palette) -> Colors {
    let panel = if p.light {
        blend(p.background, 0xffffff, 0.35)
    } else {
        blend(p.background, 0x000000, 0.22)
    };
    let field = if p.light {
        blend(p.background, 0x000000, 0.05)
    } else {
        blend(p.background, 0x000000, 0.42)
    };
    let accent = contrast(p.accent, panel, p.foreground, 0.28);
    let on_accent = if (luminance(accent) - luminance(p.background)).abs() > 0.3 {
        p.background
    } else if luminance(accent) > 0.5 {
        0x000000
    } else {
        0xffffff
    };
    Colors {
        bg: p.background,
        panel,
        field,
        fg: p.foreground,
        muted: p.muted,
        dim: blend(p.muted, panel, 0.45),
        line: blend(panel, p.foreground, 0.12),
        border: blend(p.border, p.accent, 0.35),
        accent,
        on_accent,
        sel: blend(panel, accent, 0.16),
        ok: p.success,
        warn: p.warning,
        err: p.error,
        light: p.light,
    }
}

struct P<'a> {
    cv: &'a mut Canvas,
    text: &'a mut Text,
    body: Size,
    small: Size,
}

impl P<'_> {
    fn t(&mut self, x: i32, y: i32, s: &str, c: u32) -> i32 {
        self.text.draw(self.cv, x, y, s, self.body, c)
    }
    fn ts(&mut self, x: i32, y: i32, s: &str, c: u32) -> i32 {
        self.text.draw(self.cv, x, y, s, self.small, c)
    }
    fn fit(&mut self, x: i32, y: i32, s: &str, max: i32, c: u32) -> i32 {
        let t = self.text.ellipsize(s, self.body, max.max(0));
        self.t(x, y, &t, c)
    }
    fn fit_s(&mut self, x: i32, y: i32, s: &str, max: i32, c: u32) -> i32 {
        let t = self.text.ellipsize(s, self.small, max.max(0));
        self.ts(x, y, &t, c)
    }
    fn w(&mut self, s: &str) -> i32 {
        self.text.width(s, self.body)
    }
    fn ws(&mut self, s: &str) -> i32 {
        self.text.width(s, self.small)
    }
    /// The pixel font when it has every character, else the small font,
    /// vertically centred on an 8 px line.
    fn px(&mut self, x: i32, y: i32, s: &str, c: u32) -> i32 {
        if pixfont::covers(s) {
            pixfont::draw(self.cv, x, y, s, 1, c)
        } else {
            let dy = (8 - self.small.height) / 2;
            self.ts(x, y + dy, s, c)
        }
    }
    fn px_w(&mut self, s: &str) -> i32 {
        if pixfont::covers(s) {
            pixfont::width(s, 1)
        } else {
            self.ws(s)
        }
    }
    /// A framed panel with accent corner brackets and a title row.
    fn panel(&mut self, r: Rect, c: &Colors, title: &str, summary: &str) {
        self.cv.fill(r, c.panel);
        self.cv.frame(r, c.border, 1);
        let k = 7;
        for (x, y, dx, dy) in [
            (r.x, r.y, 1, 1),
            (r.right() - 1, r.y, -1, 1),
            (r.x, r.bottom() - 1, 1, -1),
            (r.right() - 1, r.bottom() - 1, -1, -1),
        ] {
            let hx = if dx > 0 { x } else { x - k + 1 };
            let vy = if dy > 0 { y } else { y - k + 1 };
            self.cv.fill(Rect::new(hx, y, k, 1), c.accent);
            self.cv.fill(Rect::new(x, vy, 1, k), c.accent);
        }
        let ty = r.y + 8;
        let mut x = r.x + 10;
        x += self.px(x, ty, title, c.accent) + 8;
        let sw = if summary.is_empty() {
            0
        } else {
            self.px_w(summary)
        };
        let sx = r.right() - 10 - sw;
        let mut lx = x;
        while lx < sx - 8 {
            self.cv.fill(Rect::new(lx, ty + 6, 1, 1), c.line);
            lx += 3;
        }
        if sw > 0 {
            self.px(sx, ty, summary, c.muted);
        }
    }
}

fn hex(v: &Value) -> Option<u32> {
    let s = v.as_str()?.trim().trim_start_matches('#');
    (s.len() == 6)
        .then(|| u32::from_str_radix(s, 16).ok())
        .flatten()
}

pub fn paint(app: &mut SettingsApp, f: &mut Frame) {
    let c = app.c;
    let l = app.l;
    let mut p = P {
        cv: f.cv,
        text: f.text,
        body: app.body,
        small: app.small,
    };
    p.cv.fill(Rect::new(0, 0, l.w, l.h), c.bg);
    header(app, &mut p);
    sidebar(app, &mut p);
    main(app, &mut p);
    footer(app, &mut p);
    if app.app_picker.is_some() {
        app_picker(app, &mut p);
    } else if app.picker.is_some() {
        picker(app, &mut p);
    } else if app.listed.is_some() {
        listed(app, &mut p);
    } else {
        tooltip(app, &mut p);
    }
    // CRT: faint scanlines over the whole picture.
    let k = if c.light { 10 } else { 22 };
    p.cv.scanlines(Rect::new(0, 0, l.w, l.h), k);
}

fn header(app: &SettingsApp, p: &mut P) {
    let c = app.c;
    let l = app.l;
    if !p.cv.sees(&l.header) {
        return;
    }
    // The name in the pixel font at double size, with a soft phosphor
    // shadow.
    let (x, y) = (18, 12);
    pixfont::draw(
        p.cv,
        x + 1,
        y + 1,
        "NIRUSETTINGS",
        2,
        blend(c.bg, c.accent, 0.35),
    );
    let w = pixfont::draw(p.cv, x, y, "NIRUSETTINGS", 2, c.accent);
    let tag = format!("NIRUX CONTROL PANEL {}", env!("CARGO_PKG_VERSION"));
    if x + w + 16 + pixfont::width(&tag, 1) < l.search.x - 12 {
        pixfont::draw(p.cv, x + w + 14, y + 6, &tag, 1, c.muted);
    }
    // The search field.
    let s = l.search;
    let active = app.focus == Focus::Search;
    p.cv.fill(s, c.field);
    p.cv.frame(s, if active { c.accent } else { c.border }, 1);
    let ty = s.y + (s.h - app.body.height) / 2;
    pixfont::draw(p.cv, s.x + 8, s.y + (s.h - 8) / 2, "/", 1, c.accent);
    if app.query.is_empty() && !active {
        p.px(s.x + 22, s.y + (s.h - 8) / 2, "SEARCH ALL SETTINGS", c.dim);
    } else {
        let tw = p.fit(s.x + 22, ty, &app.query, s.w - 40, c.fg);
        if active && app.blink {
            p.cv.fill(
                Rect::new(
                    s.x + 23 + tw,
                    ty + 1,
                    (app.body.px * 0.55) as i32,
                    app.body.height - 2,
                ),
                c.accent,
            );
        }
    }
    // The CRT rule: an accent line that fades out toward the edges.
    let ry = l.header.bottom() - 3;
    let mid = l.w / 2;
    let mut xx = 0;
    while xx < l.w {
        let d = ((xx + 4 - mid).abs() as f32 / mid.max(1) as f32).min(1.0);
        let a = (200.0 * (1.0 - d * d)) as u32 + 20;
        p.cv.tint(Rect::new(xx, ry, 8, 1), c.accent, a);
        p.cv.tint(Rect::new(xx, ry + 1, 8, 1), c.accent, a / 4);
        xx += 8;
    }
}

fn sidebar(app: &SettingsApp, p: &mut P) {
    let c = app.c;
    let l = app.l;
    let r = l.side;
    if !p.cv.sees(&r) {
        return;
    }
    p.panel(r, &c, "SECTIONS", "");
    for (i, name) in SECTIONS.iter().enumerate() {
        let y = r.y + 26 + i as i32 * l.side_row_h;
        let row = Rect::new(r.x + 4, y, r.w - 8, l.side_row_h - 2);
        if row.bottom() > r.bottom() - 2 {
            break;
        }
        let selected = i == app.section && !app.searching();
        let focused = selected && app.focus == Focus::Side;
        let (fg, num) = if focused {
            p.cv.fill(row, c.accent);
            (c.on_accent, c.on_accent)
        } else if selected {
            p.cv.fill(row, c.sel);
            p.cv.fill(Rect::new(row.x, row.y, 3, row.h), c.accent);
            (c.fg, c.accent)
        } else {
            if app.hover_side == Some(i) {
                p.cv.tint(row, c.accent, 30);
            }
            (c.muted, c.dim)
        };
        let ty = row.y + (row.h - 8) / 2;
        pixfont::draw(p.cv, row.x + 8, ty, &format!("{}", i + 1), 1, num);
        p.px(row.x + 24, ty, &name.to_uppercase(), fg);
        if i < 6 {
            let n = app.section_modified(i);
            if n > 0 {
                let s = format!("{n}*");
                let w = pixfont::width(&s, 1);
                pixfont::draw(
                    p.cv,
                    row.right() - 8 - w,
                    ty,
                    &s,
                    1,
                    if focused { c.on_accent } else { c.accent },
                );
            }
        }
    }
    // A blinking-free cursor bar under the list: which program owns what.
    let foot_y = r.bottom() - 18;
    if foot_y > r.y + 26 + SECTIONS.len() as i32 * l.side_row_h {
        let theme = app
            .store
            .entries
            .iter()
            .position(|en| en.setting.key == "theme")
            .and_then(|e| app.current(e).value)
            .map(|v| schema::plain(&v))
            .unwrap_or_default();
        p.px(r.x + 10, foot_y, &theme.to_uppercase(), c.dim);
    }
}

fn main(app: &mut SettingsApp, p: &mut P) {
    let c = app.c;
    let l = app.l;
    let r = l.main;
    if !p.cv.sees(&r) {
        return;
    }
    let (title, summary) = if app.searching() {
        let n = app.rows.iter().filter(|r| r.selectable()).count();
        (
            format!("SEARCH \"{}\"", app.query.to_uppercase()),
            format!("{n} FOUND"),
        )
    } else if let Some(title) = app.page_title() {
        let summary = match (app.form(), app.page_table()) {
            (Some(f), _) if f.dirty() => "NOT SAVED".to_string(),
            (Some(_), _) => String::new(),
            (None, Some(t)) => cli_summary(app, t),
            _ => String::new(),
        };
        (title, summary)
    } else {
        let name = SECTIONS[app.section].to_uppercase();
        let summary = if app.section < 6 {
            let n = app.section_entries(app.section).len();
            let m = app.section_modified(app.section);
            format!("{n} SETTINGS  {m} CHANGED")
        } else {
            String::new()
        };
        (format!("[{}] {name}", app.section + 1), summary)
    };
    p.panel(r, &c, &title, &summary);
    let rows = l.rows;
    let end = (app.top + l.visible).min(app.rows.len());
    for i in app.top..end {
        let y = rows.y + (i - app.top) as i32 * l.row_h;
        let rr = Rect::new(rows.x + 2, y, rows.w - 4, l.row_h - 1);
        if !p.cv.sees(&rr) {
            continue;
        }
        let row = app.rows[i].clone();
        let selected = i == app.sel && row.selectable();
        if selected {
            let active = app.focus != Focus::Side;
            p.cv.fill(
                rr,
                if active {
                    c.sel
                } else {
                    blend(c.panel, c.sel, 0.5)
                },
            );
            p.cv.fill(
                Rect::new(rr.x, rr.y, 3, rr.h),
                if active { c.accent } else { c.dim },
            );
        } else if app.hover == Some(i) {
            p.cv.tint(rr, c.accent, 22);
        }
        let ty = rr.y + (rr.h - app.body.height) / 2;
        match row {
            Row::Head(g) => {
                let gy = rr.y + rr.h - 11;
                let w = p.px(
                    rr.x + 8,
                    gy,
                    &g.to_uppercase(),
                    blend(c.accent, c.muted, 0.35),
                );
                let mut lx = rr.x + 16 + w;
                while lx < rr.right() - 8 {
                    p.cv.fill(Rect::new(lx, gy + 4, 2, 1), c.line);
                    lx += 4;
                }
            }
            Row::Note(n) => {
                p.fit_s(
                    rr.x + 8,
                    rr.y + (rr.h - app.small.height) / 2,
                    &n,
                    rr.w - 16,
                    c.muted,
                );
            }
            Row::Info(k, v) => {
                p.fit(rr.x + 26, ty, &k, l.value_x - rr.x - 34, c.muted);
                p.fit(l.value_x, ty, &v, rr.right() - l.value_x - 8, c.fg);
            }
            Row::Act(_, label) => {
                pixfont::draw(p.cv, rr.x + 10, rr.y + (rr.h - 8) / 2, ">", 1, c.accent);
                p.fit(
                    rr.x + 26,
                    ty,
                    &label,
                    rr.w - 40,
                    if selected { c.fg } else { c.accent },
                );
            }
            Row::Key(k) => key_row(app, p, rr, ty, k, selected),
            Row::Released(j) => released_row(app, p, rr, ty, j),
            Row::Table(t) => table_row(app, p, rr, ty, t, selected),
            Row::Item(t, i) => item_row(app, p, rr, ty, t, i, selected),
            Row::Slot(t, n) => slot_row(app, p, rr, ty, t, n, selected),
            Row::Field(k) => field_row(app, p, rr, ty, k, selected),
            Row::Class => class_row(app, p, rr, ty, selected),
            Row::Extra(x) => extra_row(app, p, rr, ty, x, selected),
            Row::Set(e) => set_row(app, p, rr, ty, e, selected),
        }
    }
    // A scroll bar when there is more than fits.
    let n = app.rows.len();
    if n > l.visible {
        let track = Rect::new(r.right() - 5, rows.y, 2, rows.h);
        p.cv.fill(track, c.line);
        let h = (rows.h as usize * l.visible / n).max(12) as i32;
        let y = rows.y + ((rows.h - h) as usize * app.top / (n - l.visible).max(1)) as i32;
        p.cv.fill(Rect::new(track.x, y, 2, h), c.accent);
    }
}

/// A key combination as a key cap; returns its right edge.
fn keycap(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, x: i32, label: &str, dim: bool) -> i32 {
    let c = app.c;
    let w = p.w(label) + 12;
    let cap = Rect::new(x, rr.y + 3, w, rr.h - 6);
    p.cv.fill(cap, c.field);
    p.cv.frame(cap, c.border, 1);
    p.cv.fill(
        Rect::new(cap.x + 1, cap.bottom() - 2, cap.w - 2, 1),
        if dim { c.dim } else { c.accent },
    );
    p.t(cap.x + 6, ty, label, if dim { c.muted } else { c.fg });
    cap.right()
}

fn key_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, k: usize, selected: bool) {
    use crate::app::pages_source as src;
    let c = app.c;
    let Some(key) = app.key_view.get(k) else {
        return;
    };
    let mark_y = rr.y + (rr.h - 8) / 2;
    if key.lost {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "!", 1, c.err);
    } else if key.clash.is_some() {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "^", 1, c.warn);
    } else if matches!(src(&key.source), 1 | 2) {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "*", 1, c.accent);
    }
    let right = keycap(app, p, rr, ty, rr.x + 22, &key.label, key.lost);
    let x = (right + 12).max(rr.x + 22 + 200);
    let tag = match &key.source {
        crate::app::KeySource::Builtin => "NIRUWM".to_string(),
        crate::app::KeySource::Yours => "YOURS".into(),
        crate::app::KeySource::Changed => "CHANGED".into(),
        crate::app::KeySource::App(a) => a.to_uppercase(),
    };
    let sw = p.px_w(&tag);
    let col = if key.lost {
        c.muted
    } else if selected {
        c.fg
    } else {
        blend(c.fg, c.muted, 0.2)
    };
    p.fit(x, ty, &key.description, rr.right() - x - sw - 20, col);
    let tcol = match src(&key.source) {
        1 | 2 => c.accent,
        _ => c.dim,
    };
    p.px(rr.right() - sw - 8, mark_y, &tag, tcol);
}

fn released_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, j: usize) {
    let c = app.c;
    let Some((label, was)) = app.released_info(j) else {
        return;
    };
    let mark_y = rr.y + (rr.h - 8) / 2;
    pixfont::draw(p.cv, rr.x + 10, mark_y, "-", 1, c.warn);
    let right = keycap(app, p, rr, ty, rr.x + 22, &label, true);
    let x = (right + 12).max(rr.x + 22 + 200);
    let tag = "RELEASED";
    let sw = pixfont::width(tag, 1);
    p.fit(
        x,
        ty,
        &format!("off: {was}"),
        rr.right() - x - sw - 20,
        c.muted,
    );
    pixfont::draw(p.cv, rr.right() - sw - 8, mark_y, tag, 1, c.warn);
}

/// A count badge, then text, then an arrow: a row that opens something.
#[allow(clippy::too_many_arguments)]
fn opener(
    app: &SettingsApp,
    p: &mut P,
    rr: Rect,
    ty: i32,
    count: Option<usize>,
    text: &str,
    col: u32,
    selected: bool,
) {
    let c = app.c;
    let vx = app.l.value_x;
    let mark_y = rr.y + (rr.h - 8) / 2;
    let mut x = vx;
    if let Some(n) = count {
        let s = format!("{n}");
        let cw = pixfont::width(&s, 1);
        let b = Rect::new(vx, rr.y + (rr.h - 14) / 2, cw + 8, 14);
        p.cv.fill(b, if selected { c.accent } else { c.field });
        pixfont::draw(
            p.cv,
            b.x + 4,
            b.y + 3,
            &s,
            1,
            if selected { c.on_accent } else { c.fg },
        );
        x = b.right() + 8;
    }
    let max = rr.right() - x - 30;
    let w = p.fit(x, ty, text, max, col);
    pixfont::draw(
        p.cv,
        (x + w + 8).min(rr.right() - 16),
        mark_y,
        ">",
        1,
        if selected { c.accent } else { c.dim },
    );
}

fn table_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, t: usize, selected: bool) {
    let c = app.c;
    let l = app.l;
    let tb = app.table(t);
    let n = app.store.items(t).len();
    let mark_y = rr.y + (rr.h - 8) / 2;
    if n > 0 {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "*", 1, c.accent);
    }
    let mut label = tb.label.clone();
    if app.searching() {
        label = format!(
            "{label}  ({})",
            app.store.schemas[app.store.tables[t].schema].name
        );
    }
    p.fit(
        rr.x + 26,
        ty,
        &label,
        l.value_x - rr.x - 34,
        if selected || n > 0 {
            c.fg
        } else {
            blend(c.fg, c.muted, 0.45)
        },
    );
    let line = app.table_line(t);
    opener(
        app,
        p,
        rr,
        ty,
        Some(n),
        &line,
        if n > 0 { c.fg } else { c.muted },
        selected,
    );
}

fn item_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, t: usize, i: usize, selected: bool) {
    let c = app.c;
    let l = app.l;
    let Some(item) = app.store.items(t).get(i).cloned() else {
        return;
    };
    let (title, detail) = app.item_summary(t, &item);
    let mark_y = rr.y + (rr.h - 8) / 2;
    let mut x = rr.x + 26;
    if app.table(t).ordered {
        pixfont::draw(
            p.cv,
            rr.x + 10,
            mark_y,
            &format!("{:02}", i + 1),
            1,
            if selected { c.accent } else { c.dim },
        );
        x = rr.x + 34;
    }
    p.fit(x, ty, &title, l.value_x - x - 10, c.fg);
    opener(
        app,
        p,
        rr,
        ty,
        None,
        &detail,
        if selected { c.fg } else { c.muted },
        selected,
    );
}

fn slot_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, t: usize, n: i64, selected: bool) {
    let c = app.c;
    let l = app.l;
    let mark_y = rr.y + (rr.h - 8) / 2;
    let item = app
        .slot_index(t, n)
        .and_then(|i| app.store.items(t).get(i).cloned());
    pixfont::draw(
        p.cv,
        rr.x + 10,
        mark_y,
        &format!("{n}"),
        1,
        if item.is_some() { c.accent } else { c.dim },
    );
    let right = keycap(
        app,
        p,
        rr,
        ty,
        rr.x + 22,
        &format!("Super+Alt+{n}"),
        item.is_none(),
    );
    let x = right + 12;
    match item {
        Some(it) => {
            let (title, detail) = app.item_summary(t, &it);
            p.fit(x, ty, &title.to_uppercase(), l.value_x - x - 10, c.fg);
            opener(
                app,
                p,
                rr,
                ty,
                None,
                &detail,
                if selected { c.fg } else { c.muted },
                selected,
            );
        }
        None => {
            p.fit(x, ty, "empty", l.value_x - x - 10, c.dim);
            opener(
                app,
                p,
                rr,
                ty,
                None,
                &format!("free: send a window with Super+Alt+Shift+{n}"),
                c.dim,
                selected,
            );
        }
    }
}

fn field_row(app: &mut SettingsApp, p: &mut P, rr: Rect, ty: i32, k: usize, selected: bool) {
    let c = app.c;
    let l = app.l;
    let Some(def) = app.field_def(k).cloned() else {
        return;
    };
    let st = def.setting.clone();
    let v = app.target_value(Target::Field(k));
    let mark_y = rr.y + (rr.h - 8) / 2;
    let changed = app
        .form()
        .is_some_and(|f| f.draft.get(&st.key) != f.orig.get(&st.key));
    if st.read_only {
        // A fixed field (the slot number): its value as plain text.
        p.fit(rr.x + 26, ty, &st.label, l.value_x - rr.x - 34, c.muted);
        let text = v.as_ref().map(schema::plain).unwrap_or_default();
        p.t(l.value_x, ty, &text, c.fg);
        let tw = pixfont::width("FIXED", 1);
        pixfont::draw(
            p.cv,
            rr.right() - tw - 8,
            mark_y,
            "FIXED",
            1,
            blend(c.dim, c.panel, 0.35),
        );
        return;
    }
    if changed {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "*", 1, c.accent);
    } else if def.required && v.is_none() {
        pixfont::draw(p.cv, rr.x + 10, mark_y, "!", 1, c.warn);
    }
    let label_col = if st.read_only {
        c.muted
    } else if selected || v.is_some() {
        c.fg
    } else {
        blend(c.fg, c.muted, 0.45)
    };
    p.fit(rr.x + 26, ty, &st.label, l.value_x - rr.x - 34, label_col);
    let tag = if st.read_only {
        "FIXED"
    } else if def.required {
        "NEEDED"
    } else if v.is_some() {
        "SET"
    } else {
        "UNSET"
    };
    let tw = pixfont::width(tag, 1);
    let tag_x = rr.right() - tw - 8;
    pixfont::draw(
        p.cv,
        tag_x,
        mark_y,
        tag,
        1,
        if v.is_some() {
            c.dim
        } else {
            blend(c.dim, c.panel, 0.35)
        },
    );
    let vmax = tag_x - l.value_x - 14;
    if app
        .editor
        .as_ref()
        .is_some_and(|ed| ed.target == Target::Field(k))
    {
        draw_editor(app, p, rr, ty, vmax);
        return;
    }
    if v.is_none() {
        let s = match &st.default {
            Some(d) => format!("unset ({})", crate::app::short_value(&st, d)),
            None if def.program.as_deref() == Some("command") => {
                "none: Return chooses a program".into()
            }
            None => "unset".into(),
        };
        p.fit(l.value_x, ty, &s, vmax, c.dim);
        return;
    }
    // A command reads as the program a person picked.
    if st.kind == Kind::List && def.program.as_deref() == Some("command") {
        let argv: Vec<String> = v
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let text = nirusettings::validate::join_words(&argv);
        let fw = vmax.min(p.w(&text).max(60) + 14);
        let box_r = Rect::new(l.value_x - 4, rr.y + 3, fw + 4, rr.h - 6);
        p.cv.fill(box_r, c.field);
        p.cv.fill(
            Rect::new(box_r.x, box_r.bottom() - 1, box_r.w, 1),
            if selected { c.accent } else { c.line },
        );
        p.fit(l.value_x, ty, &text, fw - 6, c.fg);
        return;
    }
    let col = if st.read_only { c.muted } else { c.fg };
    draw_value(
        app,
        p,
        rr,
        ty,
        &st,
        v.as_ref(),
        col,
        selected && !st.read_only,
        vmax,
        None,
    );
}

fn class_row(app: &mut SettingsApp, p: &mut P, rr: Rect, ty: i32, selected: bool) {
    use nirusettings::desktop::ClassFrom;
    let c = app.c;
    let l = app.l;
    let mark_y = rr.y + (rr.h - 8) / 2;
    p.fit(
        rr.x + 26,
        ty,
        "Window class",
        l.value_x - rr.x - 34,
        if selected {
            c.fg
        } else {
            blend(c.fg, c.muted, 0.2)
        },
    );
    let cls = app.form_class();
    let (tag, tcol) = match cls.as_ref().map(|(_, f)| *f) {
        Some(Some(ClassFrom::Guess)) => ("GUESSED", c.warn),
        Some(Some(ClassFrom::Desktop)) => ("DESKTOP FILE", c.ok),
        Some(Some(ClassFrom::Nirux)) => ("NIRUX", c.ok),
        Some(_) => ("IN THE COMMAND", c.dim),
        None if app
            .form()
            .and_then(|f| f.draft.get("command"))
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty()) =>
        {
            ("MISSING", c.err)
        }
        None => ("", c.dim),
    };
    let tw = pixfont::width(tag, 1);
    let tag_x = rr.right() - tw - 8;
    pixfont::draw(p.cv, tag_x, mark_y, tag, 1, tcol);
    let vmax = tag_x - l.value_x - 14;
    if app
        .editor
        .as_ref()
        .is_some_and(|ed| ed.target == Target::Class)
    {
        draw_editor(app, p, rr, ty, vmax);
        return;
    }
    match cls {
        Some((name, _)) => {
            p.fit(l.value_x, ty, &name, vmax, c.fg);
        }
        None => {
            let empty = app
                .target_value(Target::Field(1))
                .and_then(|v| v.as_array().map(|a| a.is_empty()))
                .unwrap_or(true);
            p.fit(
                l.value_x,
                ty,
                if empty {
                    "comes with the program"
                } else {
                    "none: NIRUWM cannot find the window"
                },
                vmax,
                if empty { c.dim } else { c.err },
            );
        }
    }
}

fn extra_row(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, x: usize, selected: bool) {
    let c = app.c;
    let Some((list, _)) = &app.extras else { return };
    let e = &list[x];
    let mark_y = rr.y + (rr.h - 8) / 2;
    let installed = e.installed.is_some();
    let outdated = installed && !e.pinned.is_empty() && e.installed.as_deref() != Some(&e.pinned);
    let (mark, col) = if outdated {
        ("^", c.warn)
    } else if installed {
        ("*", c.ok)
    } else {
        (".", c.dim)
    };
    pixfont::draw(p.cv, rr.x + 10, mark_y, mark, 1, col);
    p.px(
        rr.x + 26,
        mark_y,
        &e.name.to_uppercase(),
        if installed { c.fg } else { c.muted },
    );
    let state = match (&e.installed, e.pinned.is_empty()) {
        (Some(v), _) if outdated => format!("{v} -> {}", e.pinned),
        (Some(v), _) => format!("{v} installed"),
        (None, false) => format!("{} available", e.pinned),
        (None, true) => "not installed".into(),
    };
    let scol = if outdated {
        c.warn
    } else if installed {
        c.ok
    } else {
        c.dim
    };
    // Right to left: status badge, state, then the description fills.
    let mut right = rr.right() - 8;
    let badge = e.status.to_uppercase();
    if !badge.is_empty() {
        let bw = pixfont::width(&badge, 1) + 8;
        let b = Rect::new(right - bw, mark_y - 3, bw, 14);
        p.cv.frame(b, if selected { c.accent } else { c.border }, 1);
        pixfont::draw(p.cv, b.x + 4, mark_y, &badge, 1, c.muted);
        right = b.x - 12;
    }
    let sw = p.ws(&state);
    p.ts(
        right - sw,
        rr.y + (rr.h - app.small.height) / 2,
        &state,
        scol,
    );
    right -= sw + 16;
    let dx = rr.x + 26 + 15 * 8;
    p.fit(
        dx,
        ty,
        &e.description,
        right - dx,
        if selected { c.fg } else { c.muted },
    );
}

fn set_row(app: &mut SettingsApp, p: &mut P, rr: Rect, ty: i32, e: usize, selected: bool) {
    let c = app.c;
    let l = app.l;
    let cur = app.current(e);
    let modified = app.store.modified(e, &cur);
    let st = app.store.entries[e].setting.clone();
    let file_err = app.store.file_of(e).error.is_some();
    let mark_y = rr.y + (rr.h - 8) / 2;
    let (mark, mcol) = if file_err || cur.invalid.is_some() {
        ("!", c.err)
    } else if app.since_note(e).is_some() {
        ("^", c.warn)
    } else if modified {
        ("*", c.accent)
    } else {
        ("", c.dim)
    };
    if !mark.is_empty() {
        pixfont::draw(p.cv, rr.x + 10, mark_y, mark, 1, mcol);
    }
    let mut label = st.label.clone();
    if app.searching() {
        label = format!("{label}  ({})", app.store.schema_of(e).name);
    }
    let label_col = if modified || selected {
        c.fg
    } else {
        blend(c.fg, c.muted, 0.45)
    };
    p.fit(rr.x + 26, ty, &label, l.value_x - rr.x - 34, label_col);
    // The source tag at the right edge.
    let tag = match cur.source {
        Source::File => "FILE",
        Source::Default => "DEFAULT",
        Source::Unset => "UNSET",
    };
    let tw = pixfont::width(tag, 1);
    let tag_x = rr.right() - tw - 8;
    let reset_hint = modified
        && (selected
            || app
                .hover
                .is_some_and(|h| app.rows.get(h) == Some(&Row::Set(e))));
    if reset_hint && !st.no_reset {
        pixfont::draw(p.cv, tag_x + tw - 8, mark_y, "R", 1, c.accent);
    } else {
        pixfont::draw(
            p.cv,
            tag_x,
            mark_y,
            tag,
            1,
            if cur.source == Source::File {
                c.dim
            } else {
                blend(c.dim, c.panel, 0.35)
            },
        );
    }
    let vx = l.value_x;
    let vmax = tag_x - vx - 14;
    // The text editor replaces the value while it is open.
    if app
        .editor
        .as_ref()
        .is_some_and(|ed| ed.target == Target::Entry(e))
    {
        draw_editor(app, p, rr, ty, vmax);
        return;
    }
    let vcol = if cur.source == Source::File {
        c.fg
    } else {
        c.muted
    };
    draw_value(
        app,
        p,
        rr,
        ty,
        &st,
        cur.value.as_ref(),
        vcol,
        selected,
        vmax,
        Some(e),
    );
}

/// The text editor in a row's value column.
fn draw_editor(app: &SettingsApp, p: &mut P, rr: Rect, ty: i32, vmax: i32) {
    let c = app.c;
    let vx = app.l.value_x;
    let Some(ed) = app.editor.as_ref() else {
        return;
    };
    let box_r = Rect::new(vx - 4, rr.y + 2, vmax + 8, rr.h - 4);
    p.cv.fill(box_r, c.field);
    p.cv.frame(box_r, if ed.error.is_some() { c.err } else { c.accent }, 1);
    // Scroll the text so the cursor stays in view.
    let chars: Vec<char> = ed.text.chars().collect();
    let mut start = 0;
    while start < ed.cur && p.w(&chars[start..ed.cur].iter().collect::<String>()) > vmax - 12 {
        start += 1;
    }
    let shown: String = chars[start..].iter().collect();
    p.fit(vx, ty, &shown, vmax, c.fg);
    let cx = vx + p.w(&chars[start..ed.cur].iter().collect::<String>());
    if app.blink {
        p.cv.fill(Rect::new(cx, ty + 1, 2, app.body.height - 2), c.accent);
    }
}

/// A value in a row's value column: a switch, a number with its slider,
/// a choice, a colour, a list or text. `e` is the setting's entry (for its
/// run-time choices and theme swatches); a form field has none.
#[allow(clippy::too_many_arguments)]
fn draw_value(
    app: &mut SettingsApp,
    p: &mut P,
    rr: Rect,
    ty: i32,
    st: &schema::Setting,
    v: Option<&Value>,
    vcol: u32,
    selected: bool,
    vmax: i32,
    e: Option<usize>,
) {
    let c = app.c;
    let vx = app.l.value_x;
    let mark_y = rr.y + (rr.h - 8) / 2;
    let st = st.clone();
    let v = v.cloned();
    match (st.kind, &v) {
        (_, None) => {
            let s = match st.kind {
                Kind::Colour => "theme colour",
                _ => "unset",
            };
            p.t(vx, ty, s, c.dim);
        }
        (Kind::Bool, Some(v)) => {
            let on = v.as_bool().unwrap_or(false);
            let b = Rect::new(vx, rr.y + (rr.h - 16) / 2, 46, 16);
            if on {
                p.cv.fill(b, c.accent);
                p.cv.fill(Rect::new(b.right() - 14, b.y + 2, 12, 12), c.on_accent);
                pixfont::draw(p.cv, b.x + 6, b.y + 4, "ON", 1, c.on_accent);
            } else {
                p.cv.fill(b, c.field);
                p.cv.frame(b, c.border, 1);
                p.cv.fill(Rect::new(b.x + 2, b.y + 2, 12, 12), c.dim);
                pixfont::draw(p.cv, b.x + 18, b.y + 4, "OFF", 1, c.muted);
            }
        }
        (Kind::Int | Kind::Float, Some(v)) => {
            let n = v
                .as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .unwrap_or(0.0);
            let mut s = schema::plain(v);
            if st.kind == Kind::Float && v.as_integer().is_some() {
                s = schema::fmt_float(n);
            }
            let arrows = if selected { c.accent } else { c.dim };
            pixfont::draw(p.cv, vx, mark_y, "<", 1, arrows);
            let w = p.t(vx + 16, ty, &s, vcol);
            let mut x = vx + 16 + w + 4;
            if !st.unit.is_empty() {
                x += p.ts(
                    x,
                    ty + (app.body.height - app.small.height) / 2 + 1,
                    &st.unit,
                    c.muted,
                ) + 4;
            }
            pixfont::draw(p.cv, x + 2, mark_y, ">", 1, arrows);
            // A slider for a bounded range.
            if let (Some(lo), Some(hi)) = (st.min, st.max) {
                let sx = vx + 150;
                let sw = (vmax - 150).min(160);
                if sw > 40 && hi > lo {
                    let frac = ((n - lo) / (hi - lo)).clamp(0.0, 1.0);
                    let y = rr.y + rr.h / 2 - 2;
                    p.cv.fill(Rect::new(sx, y, sw, 4), c.field);
                    let fw = (sw as f64 * frac).round() as i32;
                    p.cv.fill(
                        Rect::new(sx, y, fw, 4),
                        if selected {
                            c.accent
                        } else {
                            blend(c.accent, c.panel, 0.4)
                        },
                    );
                    for q in 1..4 {
                        p.cv.fill(Rect::new(sx + sw * q / 4, y + 4, 1, 2), c.line);
                    }
                    p.cv.fill(Rect::new(sx + fw - 1, y - 3, 3, 10), c.fg);
                }
            }
        }
        (Kind::Enum, Some(v)) => {
            let arrows = if selected { c.accent } else { c.dim };
            let label = st
                .choices
                .iter()
                .find(|ch| store::same(&ch.value, v))
                .map(|ch| ch.label.clone())
                .unwrap_or_else(|| schema::plain(v));
            pixfont::draw(p.cv, vx, mark_y, "<", 1, arrows);
            let mut x = vx + 16;
            if st.choices_from.as_deref() == Some("themes") {
                if let Some(name) = v.as_str() {
                    let sw = swatch(app, name);
                    for (k, col) in sw.iter().enumerate() {
                        let r = Rect::new(x + k as i32 * 9, rr.y + (rr.h - 12) / 2, 8, 12);
                        p.cv.fill(r, *col);
                    }
                    p.cv.frame(
                        Rect::new(
                            x - 1,
                            rr.y + (rr.h - 12) / 2 - 1,
                            9 * sw.len() as i32 + 1,
                            14,
                        ),
                        c.border,
                        1,
                    );
                    x += 9 * sw.len() as i32 + 8;
                }
            }
            let w = p.fit(x, ty, &label, vmax - (x - vx) - 60, vcol);
            pixfont::draw(p.cv, x + w + 6, mark_y, ">", 1, arrows);
            let choices: Vec<(Value, String)> = match e {
                Some(e) => app.store.choices(e, app.dynamic(e)),
                None => st
                    .choices
                    .iter()
                    .map(|ch| (ch.value.clone(), ch.label.clone()))
                    .collect(),
            };
            if let Some(i) = choices.iter().position(|(cv, _)| store::same(cv, v)) {
                let s = format!("{}/{}", i + 1, choices.len());
                let sw = pixfont::width(&s, 1);
                if x + w + 24 + sw < vx + vmax {
                    pixfont::draw(p.cv, vx + vmax - sw, mark_y, &s, 1, c.dim);
                }
            }
        }
        (Kind::Colour, Some(v)) => {
            let col = hex(v).unwrap_or(0);
            let sw = Rect::new(vx, rr.y + (rr.h - 14) / 2, 22, 14);
            p.cv.fill(sw, col);
            p.cv.frame(sw, c.border, 1);
            p.t(vx + 30, ty, &format!("#{}", schema::plain(v)), vcol);
        }
        (Kind::List, Some(v)) if st.item_kind() == Kind::Enum && st.unique => {
            let items: Vec<String> = v
                .as_array()
                .map(|a| a.iter().map(schema::plain).collect())
                .unwrap_or_default();
            let count = format!("{}", items.len());
            let cw = pixfont::width(&count, 1);
            let b = Rect::new(vx, rr.y + (rr.h - 14) / 2, cw + 8, 14);
            p.cv.fill(b, if selected { c.accent } else { c.field });
            pixfont::draw(
                p.cv,
                b.x + 4,
                b.y + 3,
                &count,
                1,
                if selected { c.on_accent } else { c.fg },
            );
            let text = if items.is_empty() {
                "none".to_string()
            } else {
                items.join(" · ")
            };
            let x = b.right() + 8;
            let w = p.fit(x, ty, &text, vmax - (x - vx) - 14, vcol);
            pixfont::draw(
                p.cv,
                x + w + 6,
                mark_y,
                ">",
                1,
                if selected { c.accent } else { c.dim },
            );
        }
        (_, Some(v)) => {
            // Text, paths and other lists in a sunken field.
            let text = match (st.kind, v) {
                (Kind::List, _) => nirusettings::validate::as_input(&st, Some(v)),
                (_, Value::String(s)) => s.clone(),
                _ => schema::plain(v),
            };
            let fw = vmax.min(p.w(&text).max(60) + 14);
            let box_r = Rect::new(vx - 4, rr.y + 3, fw + 4, rr.h - 6);
            p.cv.fill(box_r, c.field);
            p.cv.fill(
                Rect::new(box_r.x, box_r.bottom() - 1, box_r.w, 1),
                if selected { c.accent } else { c.line },
            );
            if text.is_empty() {
                p.t(vx, ty, "empty", c.dim);
            } else {
                p.fit(vx, ty, &text, fw - 6, vcol);
            }
        }
    }
}

fn cli_summary(app: &SettingsApp, t: usize) -> String {
    let tb = app.table(t);
    let n = app.store.items(t).len();
    match tb.slots {
        Some([lo, hi]) => format!("{n} OF {} SLOTS SET", hi - lo + 1),
        None => nirusettings::cli::table_summary(&app.store, t).to_uppercase(),
    }
}

/// Five colours of a theme for its swatch.
fn swatch(app: &mut SettingsApp, name: &str) -> Vec<u32> {
    if let Some(s) = app.swatches.get(name) {
        return s.clone();
    }
    let base = nirusettings::paths::config_home();
    let t = crate::gui::theme::load_from(&base, Some(name));
    let pl = t.palette;
    let s = vec![pl.background, pl.panel, pl.muted, pl.foreground, pl.accent];
    app.swatches.insert(name.to_string(), s.clone());
    s
}

fn footer(app: &SettingsApp, p: &mut P) {
    let c = app.c;
    let r = app.l.foot;
    if !p.cv.sees(&r) {
        return;
    }
    p.cv.fill(Rect::new(r.x, r.y, r.w, 1), c.border);
    let (help, info) = app.help_lines();
    let mut y = r.y + 6;
    p.fit(r.x + 4, y, &help, r.w - 8, c.fg);
    y += app.body.height + 2;
    let editor_err = app.editor.as_ref().and_then(|e| e.error.clone());
    match editor_err {
        Some(err) => {
            p.fit_s(
                r.x + 4,
                y,
                &format!("Not accepted: {err}. Esc cancels."),
                r.w - 8,
                c.err,
            );
        }
        None => {
            p.fit_s(r.x + 4, y, &info, r.w - 8, c.muted);
        }
    }
    y += app.small.height + 6;
    let (status, tone) = match &app.confirm {
        Some(cf) => (cf.text.clone(), Tone::Warn),
        None => app.status.clone(),
    };
    let (tag, col) = match tone {
        Tone::Ok => ("OK", c.ok),
        Tone::Warn => ("!!", c.warn),
        Tone::Err => ("NO", c.err),
        Tone::Info => (">>", c.accent),
    };
    let tb = Rect::new(r.x + 2, y - 2, 22, app.body.height + 2);
    p.cv.fill(tb, col);
    pixfont::draw(p.cv, tb.x + 3, tb.y + (tb.h - 8) / 2, tag, 1, c.bg);
    let mut right = r.right() - 4;
    if app.busy > 0 {
        let s = "SAVING";
        let w = pixfont::width(s, 1);
        pixfont::draw(p.cv, right - w, tb.y + (tb.h - 8) / 2, s, 1, c.accent);
        right -= w + 12;
    }
    p.fit(
        tb.right() + 8,
        y,
        &status,
        right - tb.right() - 12,
        if tone == Tone::Info { c.fg } else { col },
    );
}

fn overlay_frame(app: &SettingsApp, p: &mut P, r: Rect, title: &str) {
    let c = app.c;
    // A drop shadow, then the box.
    p.cv.tint(Rect::new(r.x + 6, r.y + 6, r.w, r.h), 0x000000, 90);
    p.panel(r, &c, title, "");
    p.cv.frame(r, c.accent, 1);
}

fn picker(app: &mut SettingsApp, p: &mut P) {
    let c = app.c;
    let Some(pk) = &app.picker else { return };
    let n = pk.items.len();
    let r = app.overlay_rect(n);
    let vis = app.overlay_visible(n);
    let mut top = pk.top.min(n.saturating_sub(vis));
    if pk.sel < top {
        top = pk.sel;
    } else if pk.sel >= top + vis {
        top = pk.sel + 1 - vis;
    }
    let target = pk.target;
    let sel = pk.sel;
    let items = pk.items.clone();
    if let Some(pk) = &mut app.picker {
        pk.top = top;
    }
    let st = app.target_setting(target);
    let title = format!("CHOOSE {}", st.label.to_uppercase());
    overlay_frame(app, p, r, &title);
    let hint = if st.preview.is_empty() {
        "RETURN CHOOSES  ESC CANCELS"
    } else {
        "RETURN CHOOSES  P PREVIEWS  ESC CANCELS"
    };
    let hw = pixfont::width(hint, 1);
    pixfont::draw(p.cv, r.right() - hw - 12, r.bottom() - 16, hint, 1, c.muted);
    let themes = st.choices_from.as_deref() == Some("themes");
    let cur = app.target_value(target);
    let rh = app.l.row_h;
    for (k, (v, label, help)) in items.iter().enumerate().skip(top).take(vis) {
        let y = r.y + 30 + (k - top) as i32 * rh;
        let rr = Rect::new(r.x + 6, y, r.w - 12, rh - 1);
        if k == sel {
            p.cv.fill(rr, c.accent);
        }
        let fg = if k == sel { c.on_accent } else { c.fg };
        let ty = rr.y + (rr.h - app.body.height) / 2;
        let now = cur.as_ref().is_some_and(|cv| store::same(cv, v));
        if now {
            pixfont::draw(p.cv, rr.x + 6, rr.y + (rr.h - 8) / 2, "*", 1, fg);
        }
        let mut x = rr.x + 22;
        if themes {
            if let Some(name) = v.as_str() {
                for (j, col) in swatch(app, name).iter().enumerate() {
                    p.cv.fill(
                        Rect::new(x + j as i32 * 7, rr.y + (rr.h - 10) / 2, 6, 10),
                        *col,
                    );
                }
                x += 44;
            }
        }
        let w = p.t(x, ty, label, fg);
        if !help.is_empty() {
            let hx = x + w + 14;
            p.fit_s(
                hx,
                rr.y + (rr.h - app.small.height) / 2,
                help,
                rr.right() - hx - 8,
                if k == sel { c.on_accent } else { c.muted },
            );
        }
    }
}

fn listed(app: &mut SettingsApp, p: &mut P) {
    let c = app.c;
    let Some(ed) = &app.listed else { return };
    let e = ed.entry;
    let st = app.store.entries[e].setting.clone();
    let all: Vec<String> = st
        .choices
        .iter()
        .map(|ch| schema::plain(&ch.value))
        .collect();
    let rows = ed.rows(&all);
    let n = rows.len();
    let r = app.overlay_rect(n);
    let vis = app.overlay_visible(n);
    let mut top = ed.top.min(n.saturating_sub(vis));
    if ed.sel < top {
        top = ed.sel;
    } else if ed.sel >= top + vis {
        top = ed.sel + 1 - vis;
    }
    let sel = ed.sel;
    let on_count = ed.on.len();
    if let Some(ed) = &mut app.listed {
        ed.top = top;
    }
    let hint = if st.ordered {
        "SPACE ON/OFF  SHIFT+UP/DOWN MOVES  ESC DONE"
    } else {
        "SPACE ON/OFF  ESC DONE"
    };
    overlay_frame(app, p, r, &st.label.to_uppercase());
    let hw = pixfont::width(hint, 1);
    pixfont::draw(p.cv, r.right() - hw - 12, r.bottom() - 16, hint, 1, c.muted);
    let count = format!("{on_count} ON");
    let cw = pixfont::width(&count, 1);
    pixfont::draw(p.cv, r.right() - cw - 12, r.y + 8, &count, 1, c.accent);
    let rh = app.l.row_h;
    for (k, (name, on)) in rows.iter().enumerate().skip(top).take(vis) {
        let y = r.y + 30 + (k - top) as i32 * rh;
        let rr = Rect::new(r.x + 6, y, r.w - 12, rh - 1);
        if k == sel {
            p.cv.fill(rr, c.accent);
        } else if k == on_count && k > 0 {
            p.cv.fill(Rect::new(rr.x, rr.y, rr.w, 1), c.line);
        }
        let fg = if k == sel {
            c.on_accent
        } else if *on {
            c.fg
        } else {
            c.muted
        };
        let my = rr.y + (rr.h - 8) / 2;
        let bx = Rect::new(rr.x + 6, my - 2, 12, 12);
        p.cv.frame(bx, fg, 1);
        if *on {
            p.cv.fill(Rect::new(bx.x + 3, bx.y + 3, 6, 6), fg);
        }
        let mut x = rr.x + 26;
        if *on && st.ordered {
            pixfont::draw(
                p.cv,
                x,
                my,
                &format!("{:02}", k + 1),
                1,
                if k == sel { c.on_accent } else { c.dim },
            );
            x += 24;
        }
        let ty = rr.y + (rr.h - app.body.height) / 2;
        let w = p.t(x, ty, name, fg);
        let help = st
            .choices
            .iter()
            .find(|ch| schema::plain(&ch.value) == *name)
            .map(|ch| ch.help.clone())
            .unwrap_or_default();
        if !help.is_empty() {
            let hx = (x + w + 14).max(rr.x + 170);
            p.fit_s(
                hx,
                rr.y + (rr.h - app.small.height) / 2,
                &help,
                rr.right() - hx - 8,
                if k == sel { c.on_accent } else { c.muted },
            );
        }
    }
}

fn app_picker(app: &mut SettingsApp, p: &mut P) {
    let c = app.c;
    let r = app.app_picker_rect();
    let vis = app.app_picker_visible();
    let list = app.picked_apps();
    let n = list.len() + 1;
    let Some(ap) = &app.app_picker else { return };
    let (sel, filter, class_only) = (ap.sel, ap.filter.clone(), ap.class_only);
    let mut top = ap.top.min(n.saturating_sub(vis));
    if sel < top {
        top = sel;
    } else if sel >= top + vis {
        top = sel + 1 - vis;
    }
    if let Some(ap) = &mut app.app_picker {
        ap.top = top;
    }
    overlay_frame(app, p, r, "CHOOSE A PROGRAM");
    let count = match &app.apps {
        Some(a) => format!("{} OF {}", list.len(), a.len()),
        None => "READING".into(),
    };
    let cw = pixfont::width(&count, 1);
    pixfont::draw(p.cv, r.right() - cw - 12, r.y + 8, &count, 1, c.accent);
    // The filter field.
    let fr = Rect::new(r.x + 10, r.y + 26, r.w - 20, 24);
    p.cv.fill(fr, c.field);
    p.cv.frame(fr, c.accent, 1);
    pixfont::draw(p.cv, fr.x + 8, fr.y + 8, "/", 1, c.accent);
    let ty = fr.y + (fr.h - app.body.height) / 2;
    if filter.is_empty() {
        p.px(
            fr.x + 22,
            fr.y + 8,
            "TYPE TO FILTER BY NAME OR COMMAND",
            c.dim,
        );
    } else {
        let w = p.t(fr.x + 22, ty, &filter, c.fg);
        p.cv.fill(
            Rect::new(
                fr.x + 23 + w,
                ty + 1,
                (app.body.px * 0.55) as i32,
                app.body.height - 2,
            ),
            c.accent,
        );
    }
    let hint = "RETURN CHOOSES  ESC CANCELS";
    let hw = pixfont::width(hint, 1);
    pixfont::draw(p.cv, r.right() - hw - 12, r.bottom() - 16, hint, 1, c.muted);
    let rh = app.l.row_h;
    for k in top..(top + vis).min(n) {
        let y = r.y + 58 + (k - top) as i32 * rh;
        let rr = Rect::new(r.x + 6, y, r.w - 12, rh - 1);
        let on = k == sel;
        if on {
            p.cv.fill(rr, c.accent);
        }
        let fg = if on { c.on_accent } else { c.fg };
        let mut_c = if on { c.on_accent } else { c.muted };
        let ty = rr.y + (rr.h - app.body.height) / 2;
        let my = rr.y + (rr.h - 8) / 2;
        if k == 0 {
            pixfont::draw(
                p.cv,
                rr.x + 10,
                my,
                ">",
                1,
                if on { c.on_accent } else { c.accent },
            );
            p.t(
                rr.x + 34,
                ty,
                if class_only {
                    "Type the window class instead"
                } else {
                    "Type a command instead"
                },
                fg,
            );
            continue;
        }
        let Some(a) = app.apps.as_ref().and_then(|x| x.get(list[k - 1]).cloned()) else {
            continue;
        };
        let ir = Rect::new(rr.x + 6, rr.y + (rr.h - 20) / 2, 20, 20);
        match app.icon(&a) {
            Some(px) => p.cv.blend_argb(ir, &px),
            None => {
                // A letter tile in the theme colours.
                p.cv.fill(ir, if on { c.on_accent } else { c.field });
                p.cv.frame(ir, if on { c.on_accent } else { c.border }, 1);
                let ch: String = a.name.chars().take(1).collect::<String>().to_uppercase();
                pixfont::draw(p.cv, ir.x + 6, ir.y + 6, &ch, 1, c.accent);
            }
        }
        let x = rr.x + 34;
        let nw = p.fit(x, ty, &a.name, (rr.w / 3).max(120), fg);
        let tag = match (&a.wm_class, a.terminal) {
            (_, true) => "TERMINAL".to_string(),
            (Some(cl), _) => format!("CLASS {}", cl.to_uppercase()),
            (None, _) => String::new(),
        };
        let tw = if tag.is_empty() { 0 } else { p.px_w(&tag) };
        if tw > 0 {
            p.px(
                rr.right() - tw - 8,
                my,
                &tag,
                if on { c.on_accent } else { c.dim },
            );
        }
        let ex = (x + nw + 14).max(x + 150);
        p.fit_s(
            ex,
            rr.y + (rr.h - app.small.height) / 2,
            &a.exec.join(" "),
            rr.right() - ex - tw - 20,
            mut_c,
        );
    }
    if app.apps.is_none() {
        p.fit_s(
            r.x + 40,
            r.y + 58 + rh + 4,
            "Reading the desktop files...",
            r.w - 60,
            c.muted,
        );
    } else if list.is_empty() {
        p.fit_s(
            r.x + 40,
            r.y + 58 + rh + 4,
            &format!("No program matches \"{filter}\"."),
            r.w - 60,
            c.muted,
        );
    }
}

fn tooltip(app: &SettingsApp, p: &mut P) {
    let c = app.c;
    let Some(h) = app.hover else { return };
    let Some(Row::Set(e)) = app.rows.get(h) else {
        return;
    };
    if app.editor.is_some() {
        return;
    }
    let (mx, my) = app.mouse;
    if mx >= app.l.value_x {
        return;
    }
    let st = &app.store.entries[*e].setting;
    if st.help.is_empty() {
        return;
    }
    let maxw = 380;
    let mut lines = p.text.wrap(&st.help, app.small, maxw);
    lines.truncate(4);
    let key = format!("{}  [{}]", st.key, app.store.schema_of(*e).component);
    let mut w = p.ws(&key);
    for l in &lines {
        w = w.max(p.ws(l));
    }
    w += 16;
    let lh = app.small.height + 2;
    let hgt = (lines.len() as i32 + 1) * lh + 12;
    let b = Rect::new(0, 0, app.l.w, app.l.h);
    let mut x = mx + 16;
    let mut y = my + 18;
    if x + w > b.right() - 4 {
        x = mx - 16 - w;
    }
    if y + hgt > app.l.foot.y - 2 {
        y = my - 10 - hgt;
    }
    let r = Rect::new(x.max(4), y.max(4), w, hgt);
    p.cv.tint(Rect::new(r.x + 4, r.y + 4, r.w, r.h), 0x000000, 80);
    p.cv.fill(r, blend(c.panel, c.fg, 0.07));
    p.cv.frame(r, c.accent, 1);
    let mut ty = r.y + 6;
    p.ts(r.x + 8, ty, &key, c.accent);
    ty += lh;
    for l in &lines {
        p.ts(r.x + 8, ty, l, c.fg);
        ty += lh;
    }
}
