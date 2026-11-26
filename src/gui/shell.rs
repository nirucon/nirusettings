//! The event loop: one X window, live theme reload, and no work while
//! idle. It sleeps in poll() on the X connection, an eventfd the worker
//! thread writes to, and inotify on the configuration folders; there is no
//! regular wake-up at all (the app asks for a tick only while it waits). Redraws go tile by tile: the app
//! names the rectangles that changed, and each is drawn into the shared
//! memory tile and sent. Hidden or fully covered, nothing is drawn.
use super::canvas::{Canvas, Rect};
use super::text::Text;
use super::theme::{self, Theme};
use super::x::{Kind, Pointer, Res, Spec, X};
use std::os::fd::AsRawFd;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{KeyButMask, Visibility};
use x11rb::protocol::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Redraw {
    None,
    /// Only what the app lists in `dirty`.
    Partial,
    Full,
}

#[derive(Clone, Debug)]
pub struct KeyEv {
    pub sym: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub ch: Option<char>,
}

#[derive(Clone, Copy, Debug)]
pub enum Mouse {
    Press { x: i32, y: i32, button: u8 },
    Motion { x: i32, y: i32 },
    Leave,
}

pub struct Frame<'a> {
    pub cv: &'a mut Canvas,
    pub text: &'a mut Text,
    pub theme: &'a Theme,
}

pub trait App {
    /// Draw everything that falls into the canvas's tile.
    fn paint(&mut self, f: &mut Frame);
    /// The rectangles that changed since the last paint.
    fn dirty(&mut self, out: &mut Vec<Rect>);
    fn resize(&mut self, w: i32, h: i32);
    /// Called before every redraw: colours, sizes and layout.
    fn prepare(&mut self, text: &mut Text, theme: &Theme);
    fn key(&mut self, k: &KeyEv) -> Redraw;
    fn mouse(&mut self, m: Mouse) -> Redraw;
    /// The worker signalled the waker.
    fn poll(&mut self) -> Redraw;
    fn next_tick(&self) -> Option<Instant>;
    fn tick(&mut self) -> Redraw;
    /// The window became visible or hidden (unmapped, another workspace,
    /// fully covered).
    fn visible(&mut self, v: bool);
    fn title(&self) -> &str;
    fn pointer(&self) -> Pointer {
        Pointer::Arrow
    }
    fn wants_quit(&self) -> bool;
    /// A new size and position the app wants (widgets fit their content).
    fn take_geometry(&mut self) -> Option<Geometry> {
        None
    }
    fn wm_name(&mut self, _name: Option<String>) {}
    fn quit(&mut self) {}
    /// A watched folder changed (a configuration file was written).
    fn files_changed(&mut self) -> Redraw {
        Redraw::None
    }
    /// A second start asked this window to show something (a section).
    fn request(&mut self, _what: &str) -> Redraw {
        Redraw::None
    }
    /// The window gained or lost the keyboard focus.
    fn focused(&mut self, _on: bool) -> Redraw {
        Redraw::None
    }
}

/// A window position (when it should move) and size.
pub type Geometry = (Option<(i32, i32)>, u32, u32);

/// Wakes the event loop from another thread.
#[derive(Clone, Copy, Debug)]
pub struct Waker(i32);

impl Waker {
    pub fn wake(&self) {
        let one: u64 = 1;
        unsafe { libc::write(self.0, &one as *const u64 as *const libc::c_void, 8) };
    }
}

static WAKE_FD: OnceLock<i32> = OnceLock::new();

pub fn waker() -> Waker {
    Waker(
        *WAKE_FD
            .get_or_init(|| unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) }),
    )
}

/// Window icons in several sizes: (width, height, ARGB pixels).
pub type Icons = Vec<(u32, u32, Vec<u32>)>;

pub struct Opts {
    pub title: &'static str,
    pub class: &'static str,
    pub size: (u16, u16),
    pub pos: (i32, i32),
    pub min: (i32, i32),
    pub kind: Kind,
    pub icon: fn(&theme::Palette) -> Icons,
    /// More folders to watch (the app's files_changed is called).
    pub watch: Vec<std::path::PathBuf>,
    /// The selection that marks the one running instance.
    pub claim: Option<&'static str>,
}

/// The property a second start sets on the running window.
pub const REQUEST_PROP: &str = "_NIRUSETTINGS_OPEN";

fn watch(ino: i32, extra: &[std::path::PathBuf]) {
    use std::os::unix::ffi::OsStrExt;
    for dir in theme::watch_dirs().iter().chain(extra) {
        if let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) {
            unsafe {
                libc::inotify_add_watch(
                    ino,
                    c.as_ptr(),
                    libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE | libc::IN_DELETE,
                )
            };
        }
    }
}

fn drain(fd: i32) {
    let mut buf = [0u8; 4096];
    while unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) } > 0 {}
}

/// The character a keysym types: Latin-1 directly, Unicode keysyms
/// (0x01000000 + code point), and nothing for function keys.
pub fn keysym_char(ks: u32) -> Option<char> {
    match ks {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(ks),
        0x0100_0000..=0x0110_ffff => char::from_u32(ks - 0x0100_0000),
        _ => None,
    }
}

pub fn trace_on() -> bool {
    static T: OnceLock<bool> = OnceLock::new();
    *T.get_or_init(|| std::env::var_os("NIRUSETTINGS_TRACE").is_some())
}

/// Allocations made so far (counted by the binary's allocator).
pub static ALLOCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Screen size before the window exists (to place widgets).
pub fn screen_size() -> Option<(i32, i32)> {
    let (c, n) = x11rb::connect(None).ok()?;
    let s = &c.setup().roots[n];
    Some((s.width_in_pixels as i32, s.height_in_pixels as i32))
}

pub fn run(app: &mut dyn App, o: Opts, started: Instant) -> Res<()> {
    let mut x = X::open(&Spec {
        pos: o.pos,
        size: o.size,
        min: o.min,
        title: o.title,
        class: o.class,
        kind: o.kind,
    })?;
    let request_atom = x.claim(o.claim, REQUEST_PROP)?;
    app.wm_name(x.wm_name());
    let mut theme: Theme = theme::load(x.root_theme().as_deref());
    let mut text = Text::new(theme.font_path.as_deref())?;
    let _ = x.set_icon(&(o.icon)(&theme.palette));
    let mut cv = x.canvas();
    let (mut w, mut h) = (o.size.0 as i32, o.size.1 as i32);
    app.resize(w, h);
    x.map()?;
    let widget = o.kind != Kind::Normal;
    let wake = waker().0;
    let ino = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if ino >= 0 {
        watch(ino, &o.watch);
    }
    let theme_atom = x.atoms.niruwm_theme;
    let xfd = x.c.stream().as_raw_fd();
    let mut redraw = Redraw::Full;
    let mut shown = false;
    let mut reload: Option<Instant> = None;
    let mut mapped = false;
    let mut obscured = false;
    let mut was_visible = false;
    let mut title = String::new();
    let mut frames = 0u32;
    let mut tiles_sent = 0u32;
    let mut fps_at = Instant::now();
    let mut allocs_at = ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
    let mut dirty: Vec<Rect> = Vec::with_capacity(64);
    let mut tiles: Vec<Rect> = Vec::with_capacity(64);
    loop {
        while let Some(ev) = x.c.poll_for_event()? {
            match ev {
                Event::Expose(e) if e.count == 0 => redraw = Redraw::Full,
                Event::ConfigureNotify(e) if e.window == x.win => {
                    let (nw, nh) = (e.width as i32, e.height as i32);
                    if nw != w || nh != h {
                        w = nw;
                        h = nh;
                        app.resize(w, h);
                        redraw = Redraw::Full;
                    }
                }
                Event::MapNotify(_) => {
                    mapped = true;
                    redraw = Redraw::Full;
                    if widget {
                        x.lower();
                    }
                }
                Event::UnmapNotify(_) => mapped = false,
                Event::VisibilityNotify(e) => {
                    obscured = e.state == Visibility::FULLY_OBSCURED;
                    if widget && e.state != Visibility::FULLY_OBSCURED {
                        // NIRUWM puts a desktop window at the bottom once;
                        // keep it there.
                        x.lower();
                    }
                }
                Event::KeyPress(e) => {
                    let ks = x.keysym(e.detail, e.state);
                    if (0xffe1..=0xffee).contains(&ks) || ks == 0xfe03 {
                        continue;
                    }
                    let k = KeyEv {
                        sym: ks,
                        ctrl: e.state.contains(KeyButMask::CONTROL),
                        shift: e.state.contains(KeyButMask::SHIFT),
                        ch: keysym_char(ks),
                    };
                    redraw = redraw.max(app.key(&k));
                }
                Event::ButtonPress(e) => {
                    redraw = redraw.max(app.mouse(Mouse::Press {
                        x: e.event_x as i32,
                        y: e.event_y as i32,
                        button: e.detail,
                    }));
                }
                Event::MotionNotify(e) => {
                    redraw = redraw.max(app.mouse(Mouse::Motion {
                        x: e.event_x as i32,
                        y: e.event_y as i32,
                    }));
                    x.set_pointer(app.pointer());
                }
                Event::LeaveNotify(_) => redraw = redraw.max(app.mouse(Mouse::Leave)),
                Event::ClientMessage(e) => {
                    if e.format == 32 && e.data.as_data32()[0] == x.atoms.wm_delete {
                        app.quit();
                        return Ok(());
                    }
                }
                Event::PropertyNotify(e) if e.window == x.root && e.atom == theme_atom => {
                    reload = Some(Instant::now());
                }
                Event::PropertyNotify(e) if e.window == x.win && Some(e.atom) == request_atom => {
                    if let Some(what) = x.take_string(e.atom) {
                        redraw = redraw.max(app.request(&what));
                    }
                }
                Event::FocusIn(_) => redraw = redraw.max(app.focused(true)),
                Event::FocusOut(_) => redraw = redraw.max(app.focused(false)),
                _ => {}
            }
        }
        let visible = mapped && !obscured;
        if visible != was_visible {
            was_visible = visible;
            app.visible(visible);
            if trace_on() {
                eprintln!("trace|visible|{visible}");
            }
            if visible {
                redraw = Redraw::Full;
            }
        }
        if app.wants_quit() {
            app.quit();
            return Ok(());
        }
        if let Some((pos, gw, gh)) = app.take_geometry() {
            x.move_resize(pos, gw, gh);
        }
        let now = Instant::now();
        if reload.is_some_and(|t| now >= t) {
            reload = None;
            let new = theme::load(x.root_theme().as_deref());
            if new != theme {
                if new.font_path != theme.font_path || new.font_px != theme.font_px {
                    if let Ok(t) = Text::new(new.font_path.as_deref()) {
                        text = t;
                    }
                }
                theme = new;
                app.resize(w, h);
                let _ = x.set_icon(&(o.icon)(&theme.palette));
                if trace_on() {
                    eprintln!(
                        "trace|theme|{}|{:06x}",
                        theme.name, theme.palette.background
                    );
                }
                redraw = Redraw::Full;
            }
        }
        if app.next_tick().is_some_and(|t| now >= t) {
            redraw = redraw.max(app.tick());
        }
        if redraw != Redraw::None && (visible || !shown) {
            app.prepare(&mut text, &theme);
            if let Some((pos, gw, gh)) = app.take_geometry() {
                x.move_resize(pos, gw, gh);
            }
            dirty.clear();
            if redraw == Redraw::Full || !shown {
                dirty.push(Rect::new(0, 0, w, h));
                // Forget the partial list: everything is drawn.
                let mut scratch = std::mem::take(&mut tiles);
                app.dirty(&mut scratch);
                scratch.clear();
                tiles = scratch;
            } else {
                app.dirty(&mut dirty);
            }
            redraw = Redraw::None;
            tiles.clear();
            for r in &dirty {
                let r = r.intersect(&Rect::new(0, 0, w, h));
                X::tiles(r, &mut tiles);
            }
            let t_paint = Instant::now();
            let a0 = ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
            let mut paint_us = 0u128;
            let mut paint_allocs = 0u64;
            let mut offset = 0usize;
            let mut syncs = 0;
            for t in &tiles {
                let px = (t.w * t.h).max(0) as usize;
                // Pack tiles into the shared segment; wait for the server
                // only when it is full.
                if x.has_shm() && offset + px > super::x::TILE_PIXELS {
                    x.sync()?;
                    syncs += 1;
                    offset = 0;
                }
                let mut packed = if x.has_shm() {
                    x.canvas_at(offset)
                } else {
                    None
                };
                let cvr = packed.as_mut().unwrap_or(&mut cv);
                cvr.set_tile(*t);
                let tp = Instant::now();
                let ap = ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
                {
                    let mut f = Frame {
                        cv: cvr,
                        text: &mut text,
                        theme: &theme,
                    };
                    app.paint(&mut f);
                }
                paint_us += tp.elapsed().as_micros();
                paint_allocs += ALLOCS.load(std::sync::atomic::Ordering::Relaxed) - ap;
                x.present(cvr, offset)?;
                offset += px;
                tiles_sent += 1;
            }
            if x.has_shm() && !tiles.is_empty() {
                x.sync()?;
                syncs += 1;
            }
            if trace_on() {
                eprintln!(
                    "trace|frame|tiles|{}|syncs|{syncs}|paint_us|{paint_us}|total_us|{}|paint_allocs|{paint_allocs}|allocs|{}",
                    tiles.len(),
                    t_paint.elapsed().as_micros(),
                    ALLOCS.load(std::sync::atomic::Ordering::Relaxed) - a0
                );
            }
            frames += 1;
            if app.title() != title {
                let _ = x.set_title(app.title());
                title.clear();
                title.push_str(app.title());
            }
            if !shown {
                shown = true;
                if std::env::var_os("NIRUSETTINGS_TIMING").is_some() {
                    eprintln!(
                        "nirusettings: first frame after {:.1} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
            continue;
        } else if !visible {
            redraw = Redraw::None;
        }
        if trace_on() && fps_at.elapsed() >= Duration::from_secs(1) {
            let a = ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
            eprintln!(
                "trace|fps|{frames}|tiles|{tiles_sent}|allocs|{}",
                a - allocs_at
            );
            allocs_at = a;
            frames = 0;
            tiles_sent = 0;
            fps_at = Instant::now();
        }
        x.c.flush()?;
        let mut timeout: i32 = -1;
        let mut at = |t: Instant| {
            let ms = t.saturating_duration_since(Instant::now()).as_millis() as i32;
            timeout = if timeout < 0 { ms } else { timeout.min(ms) };
        };
        if let Some(t) = reload {
            at(t);
        }
        if let Some(t) = app.next_tick() {
            at(t.max(Instant::now()));
        }
        let mut fds = [
            libc::pollfd {
                fd: xfd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: ino,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = if ino >= 0 { 3 } else { 2 };
        unsafe { libc::poll(fds.as_mut_ptr(), n, timeout) };
        if fds[1].revents & libc::POLLIN != 0 {
            drain(wake);
            redraw = redraw.max(app.poll());
        }
        if n == 3 && fds[2].revents & libc::POLLIN != 0 {
            drain(ino);
            watch(ino, &o.watch);
            reload = Some(Instant::now() + Duration::from_millis(150));
            redraw = redraw.max(app.files_changed());
        }
    }
}
