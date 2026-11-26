//! The X11 side: one window (normal, a desktop widget, or override
//! redirect), a keyboard map, mouse cursors, and sending tiles of the
//! picture with MIT-SHM (one shared segment the size of the largest tile)
//! or, without it, `PutImage` in slices that fit the request size.
use super::canvas::{Canvas, Rect};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::properties::{WmHints, WmSizeHints};
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Pixels of the largest tile (1 MiB of memory).
pub const TILE_PIXELS: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Normal,
    /// _NET_WM_WINDOW_TYPE_DESKTOP, sticky, below, skip taskbar and pager,
    /// no input focus: a conky-like widget.
    Desktop,
    /// Override redirect: no window manager at all; it lowers itself.
    Override,
}

pub struct Atoms {
    pub wm_protocols: Atom,
    pub wm_delete: Atom,
    pub net_wm_name: Atom,
    pub net_wm_icon: Atom,
    pub net_wm_pid: Atom,
    pub utf8: Atom,
    pub niruwm_theme: Atom,
    pub net_active: Atom,
    pub net_wm_window_type: Atom,
    pub net_wm_window_type_desktop: Atom,
    pub net_wm_state: Atom,
    pub net_wm_state_sticky: Atom,
    pub net_wm_state_below: Atom,
    pub net_wm_state_skip_taskbar: Atom,
    pub net_wm_state_skip_pager: Atom,
    pub net_wm_desktop: Atom,
    pub net_supporting_wm_check: Atom,
    pub motif_wm_hints: Atom,
}

struct Shm {
    seg: u32,
    addr: *mut u8,
}

/// Mouse pointer shapes from the core cursor font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Arrow,
    Hand,
}

pub struct X {
    pub c: RustConnection,
    pub root: Window,
    pub win: Window,
    pub depth: u8,
    pub screen: (i32, i32),
    pub kind: Kind,
    gc: Gcontext,
    pub atoms: Atoms,
    shm: Option<Shm>,
    min_keycode: u8,
    per: u8,
    keysyms: Vec<u32>,
    cursors: [u32; 2],
    pointer: Pointer,
}

pub struct Spec<'a> {
    pub pos: (i32, i32),
    pub size: (u16, u16),
    pub min: (i32, i32),
    pub title: &'a str,
    pub class: &'a str,
    pub kind: Kind,
}

impl X {
    pub fn open(sp: &Spec) -> Res<Self> {
        let (c, screen) = x11rb::connect(None)?;
        Self::open_with(c, screen, sp)
    }

    /// Open the window on a connection made earlier (one connection per
    /// start: a second connect costs as much as the rest of the start).
    pub fn open_with(c: RustConnection, screen: usize, sp: &Spec) -> Res<Self> {
        let s = &c.setup().roots[screen];
        let root = s.root;
        let depth = s.root_depth;
        let visual = s.root_visual;
        let scr = (s.width_in_pixels as i32, s.height_in_pixels as i32);
        // Ask for all atoms first and wait once: one round trip.
        let names = [
            "WM_PROTOCOLS",
            "WM_DELETE_WINDOW",
            "_NET_WM_NAME",
            "_NET_WM_ICON",
            "_NET_WM_PID",
            "UTF8_STRING",
            "_NIRUWM_THEME",
            "_NET_ACTIVE_WINDOW",
            "_NET_WM_WINDOW_TYPE",
            "_NET_WM_WINDOW_TYPE_DESKTOP",
            "_NET_WM_STATE",
            "_NET_WM_STATE_STICKY",
            "_NET_WM_STATE_BELOW",
            "_NET_WM_STATE_SKIP_TASKBAR",
            "_NET_WM_STATE_SKIP_PAGER",
            "_NET_WM_DESKTOP",
            "_NET_SUPPORTING_WM_CHECK",
            "_MOTIF_WM_HINTS",
        ];
        let cookies: Vec<_> = names
            .iter()
            .map(|n| c.intern_atom(false, n.as_bytes()))
            .collect::<Result<_, _>>()?;
        let mut g = Vec::with_capacity(names.len());
        for ck in cookies {
            g.push(ck.reply()?.atom);
        }
        let atoms = Atoms {
            wm_protocols: g[0],
            wm_delete: g[1],
            net_wm_name: g[2],
            net_wm_icon: g[3],
            net_wm_pid: g[4],
            utf8: g[5],
            niruwm_theme: g[6],
            net_active: g[7],
            net_wm_window_type: g[8],
            net_wm_window_type_desktop: g[9],
            net_wm_state: g[10],
            net_wm_state_sticky: g[11],
            net_wm_state_below: g[12],
            net_wm_state_skip_taskbar: g[13],
            net_wm_state_skip_pager: g[14],
            net_wm_desktop: g[15],
            net_supporting_wm_check: g[16],
            motif_wm_hints: g[17],
        };
        let win = c.generate_id()?;
        let mut events = EventMask::EXPOSURE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::LEAVE_WINDOW
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::VISIBILITY_CHANGE;
        if sp.kind == Kind::Normal {
            events |= EventMask::KEY_PRESS | EventMask::FOCUS_CHANGE | EventMask::PROPERTY_CHANGE;
        }
        let mut aux = CreateWindowAux::new()
            .background_pixel(s.black_pixel)
            .bit_gravity(Gravity::NORTH_WEST)
            .event_mask(events);
        if sp.kind == Kind::Override {
            aux = aux.override_redirect(1);
        }
        c.create_window(
            depth,
            win,
            root,
            sp.pos.0 as i16,
            sp.pos.1 as i16,
            sp.size.0,
            sp.size.1,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &aux,
        )?;
        // The theme property on the root window changes when the user picks
        // another NIRU theme.
        c.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let wm_class = format!("{}\0{}\0", sp.class, sp.class);
        c.change_property8(
            PropMode::REPLACE,
            win,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            wm_class.as_bytes(),
        )?;
        c.change_property32(
            PropMode::REPLACE,
            win,
            atoms.wm_protocols,
            AtomEnum::ATOM,
            &[atoms.wm_delete],
        )?;
        c.change_property32(
            PropMode::REPLACE,
            win,
            atoms.net_wm_pid,
            AtomEnum::CARDINAL,
            &[std::process::id()],
        )?;
        let mut hints = WmSizeHints::new();
        hints.min_size = Some(sp.min);
        if sp.kind != Kind::Normal {
            hints.position = Some((
                x11rb::properties::WmSizeHintsSpecification::UserSpecified,
                sp.pos.0,
                sp.pos.1,
            ));
            hints.size = Some((
                x11rb::properties::WmSizeHintsSpecification::UserSpecified,
                sp.size.0 as i32,
                sp.size.1 as i32,
            ));
        }
        hints.set_normal_hints(&c, win)?;
        if sp.kind != Kind::Normal {
            // A widget: desktop type, on every workspace, below everything,
            // out of task bars and pagers, never focused, no decorations.
            let a = &atoms;
            c.change_property32(
                PropMode::REPLACE,
                win,
                a.net_wm_window_type,
                AtomEnum::ATOM,
                &[a.net_wm_window_type_desktop],
            )?;
            c.change_property32(
                PropMode::REPLACE,
                win,
                a.net_wm_state,
                AtomEnum::ATOM,
                &[
                    a.net_wm_state_sticky,
                    a.net_wm_state_below,
                    a.net_wm_state_skip_taskbar,
                    a.net_wm_state_skip_pager,
                ],
            )?;
            c.change_property32(
                PropMode::REPLACE,
                win,
                a.net_wm_desktop,
                AtomEnum::CARDINAL,
                &[0xffff_ffff],
            )?;
            let mut wh = WmHints::new();
            wh.input = Some(false);
            wh.set(&c, win)?;
            // flags=decorations, decorations=0
            c.change_property32(
                PropMode::REPLACE,
                win,
                a.motif_wm_hints,
                a.motif_wm_hints,
                &[2, 0, 0, 0, 0],
            )?;
        }
        let gc = c.generate_id()?;
        c.create_gc(gc, win, &CreateGCAux::new().graphics_exposures(0))?;
        let setup = c.setup();
        let (min_keycode, max_keycode) = (setup.min_keycode, setup.max_keycode);
        let map = c
            .get_keyboard_mapping(min_keycode, max_keycode - min_keycode + 1)?
            .reply()?;
        let cursors = make_cursors(&c);
        let mut x = X {
            c,
            root,
            win,
            depth,
            screen: scr,
            kind: sp.kind,
            gc,
            atoms,
            shm: None,
            min_keycode,
            per: map.keysyms_per_keycode,
            keysyms: map.keysyms,
            cursors,
            pointer: Pointer::Arrow,
        };
        x.make_shm();
        x.set_title(sp.title)?;
        Ok(x)
    }

    fn make_shm(&mut self) {
        let ok = std::env::var_os("NIRU_NO_SHM").is_none()
            && self
                .c
                .extension_information(x11rb::protocol::shm::X11_EXTENSION_NAME)
                .ok()
                .flatten()
                .is_some()
            && self
                .c
                .shm_query_version()
                .ok()
                .and_then(|r| r.reply().ok())
                .is_some();
        if !ok {
            return;
        }
        let size = TILE_PIXELS * 4;
        unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600);
            if id < 0 {
                return;
            }
            let addr = libc::shmat(id, std::ptr::null(), 0);
            if addr as isize == -1 {
                libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                return;
            }
            let Ok(seg) = self.c.generate_id() else {
                return;
            };
            let attached = self
                .c
                .shm_attach(seg, id as u32, false)
                .ok()
                .and_then(|ck| ck.check().ok())
                .is_some();
            // Removed now: the segment lives until both sides detach, so a
            // crash cannot leak it.
            libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
            if !attached {
                libc::shmdt(addr);
                return;
            }
            self.shm = Some(Shm {
                seg,
                addr: addr as *mut u8,
            });
        }
    }

    /// The canvas to draw tiles into: the shared segment, or memory of its
    /// own without MIT-SHM.
    pub fn canvas(&self) -> Canvas {
        match &self.shm {
            Some(s) => unsafe { Canvas::from_raw(s.addr as *mut u32, TILE_PIXELS) },
            None => Canvas::with_capacity(TILE_PIXELS),
        }
    }

    /// A canvas over the shared segment from pixel `offset` on: several
    /// tiles of one frame are packed into the segment and sent with one
    /// round trip at the end (see [`X::sync`]).
    pub fn canvas_at(&self, offset: usize) -> Option<Canvas> {
        let s = self.shm.as_ref()?;
        if offset >= TILE_PIXELS {
            return None;
        }
        Some(unsafe { Canvas::from_raw((s.addr as *mut u32).add(offset), TILE_PIXELS - offset) })
    }

    /// Wait until the server has read every tile sent so far.
    pub fn sync(&mut self) -> Res<()> {
        self.c.get_input_focus()?.reply()?;
        Ok(())
    }

    /// Own the single-instance selection `name` with this window and
    /// return the atom of the request property `prop`.
    pub fn claim(&self, name: Option<&str>, prop: &str) -> Res<Option<Atom>> {
        let Some(name) = name else { return Ok(None) };
        let sel = self.c.intern_atom(false, name.as_bytes())?.reply()?.atom;
        self.c
            .set_selection_owner(self.win, sel, x11rb::CURRENT_TIME)?;
        let p = self.c.intern_atom(false, prop.as_bytes())?.reply()?.atom;
        Ok(Some(p))
    }

    /// Read and delete a string property of the window.
    pub fn take_string(&self, atom: Atom) -> Option<String> {
        let r = self
            .c
            .get_property(true, self.win, atom, AtomEnum::ANY, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        if r.value.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&r.value).into_owned())
    }

    pub fn has_shm(&self) -> bool {
        self.shm.is_some()
    }

    pub fn set_title(&self, title: &str) -> Res<()> {
        self.c.change_property8(
            PropMode::REPLACE,
            self.win,
            self.atoms.net_wm_name,
            self.atoms.utf8,
            title.as_bytes(),
        )?;
        self.c.change_property8(
            PropMode::REPLACE,
            self.win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        )?;
        Ok(())
    }

    /// `_NET_WM_ICON`: width, height, then ARGB pixels, for every size.
    pub fn set_icon(&self, sizes: &[(u32, u32, Vec<u32>)]) -> Res<()> {
        let mut data = Vec::new();
        for (w, h, px) in sizes {
            data.push(*w);
            data.push(*h);
            data.extend_from_slice(px);
        }
        self.c.change_property32(
            PropMode::REPLACE,
            self.win,
            self.atoms.net_wm_icon,
            AtomEnum::CARDINAL,
            &data,
        )?;
        Ok(())
    }

    pub fn map(&self) -> Res<()> {
        self.c.map_window(self.win)?;
        if self.kind != Kind::Normal {
            self.lower();
        }
        self.c.flush()?;
        Ok(())
    }

    /// Put the window at the bottom of the stack (widgets).
    pub fn lower(&self) {
        let _ = self.c.configure_window(
            self.win,
            &ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
        );
    }

    pub fn move_resize(&self, pos: Option<(i32, i32)>, w: u32, h: u32) {
        let mut aux = ConfigureWindowAux::new().width(w).height(h);
        if let Some((x, y)) = pos {
            aux = aux.x(x).y(y);
        }
        let _ = self.c.configure_window(self.win, &aux);
        if self.kind != Kind::Normal {
            // Keep the size hints in step so a manager keeps the size.
            let mut hints = WmSizeHints::new();
            hints.min_size = Some((w as i32, h as i32));
            hints.max_size = Some((w as i32, h as i32));
            if let Some((x, y)) = pos {
                hints.position = Some((
                    x11rb::properties::WmSizeHintsSpecification::UserSpecified,
                    x,
                    y,
                ));
            }
            let _ = hints.set_normal_hints(&self.c, self.win);
        }
    }

    pub fn set_pointer(&mut self, p: Pointer) {
        if p == self.pointer {
            return;
        }
        self.pointer = p;
        let cur = self.cursors[p as usize];
        if cur != 0 {
            let _ = self
                .c
                .change_window_attributes(self.win, &ChangeWindowAttributesAux::new().cursor(cur));
        }
    }

    fn prop(&self, w: Window, atom: Atom, max: u32) -> Option<GetPropertyReply> {
        self.c
            .get_property(false, w, atom, AtomEnum::ANY, 0, max)
            .ok()?
            .reply()
            .ok()
    }

    pub fn root_theme(&self) -> Option<String> {
        let r = self.prop(self.root, self.atoms.niruwm_theme, 64)?;
        let s = String::from_utf8(r.value).ok()?;
        (!s.is_empty()).then_some(s)
    }

    /// The window manager's name (EWMH supporting window), asked once.
    pub fn wm_name(&self) -> Option<String> {
        let r = self.prop(self.root, self.atoms.net_supporting_wm_check, 1)?;
        let w = r.value32()?.next()?;
        let n = self.prop(w, self.atoms.net_wm_name, 64)?;
        let s = String::from_utf8_lossy(&n.value).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// The keysym of a key, with Shift (column 1) or AltGr (columns 4/5).
    /// The keyboard mapping: the first keycode, keysyms per keycode, and
    /// every keysym.
    pub fn keymap(&self) -> (u8, u8, &[u32]) {
        (self.min_keycode, self.per, &self.keysyms)
    }

    /// Read the keyboard mapping again (the layout changed).
    pub fn reload_keymap(&mut self) {
        let setup = self.c.setup();
        let (lo, hi) = (setup.min_keycode, setup.max_keycode);
        if let Ok(Ok(map)) = self
            .c
            .get_keyboard_mapping(lo, hi - lo + 1)
            .map(|c| c.reply())
        {
            self.min_keycode = lo;
            self.per = map.keysyms_per_keycode;
            self.keysyms = map.keysyms;
        }
    }

    pub fn keysym(&self, keycode: u8, state: KeyButMask) -> u32 {
        let idx = (keycode.saturating_sub(self.min_keycode)) as usize * self.per as usize;
        let shift = state.contains(KeyButMask::SHIFT) ^ state.contains(KeyButMask::LOCK);
        let altgr = state.contains(KeyButMask::MOD5);
        let col = match (altgr, shift) {
            (true, false) => 4,
            (true, true) => 5,
            (false, true) => 1,
            (false, false) => 0,
        };
        let get = |c: usize| self.keysyms.get(idx + c).copied().unwrap_or(0);
        let k = get(col);
        if k != 0 {
            return k;
        }
        let base = get(if altgr { 4 } else { 0 });
        if base != 0 {
            if shift && (0x61..=0x7a).contains(&base) {
                return base - 0x20;
            }
            return base;
        }
        get(0)
    }

    /// Send the canvas's tile, drawn at pixel `offset` of the shared
    /// segment. With MIT-SHM the caller syncs before that memory is drawn
    /// into again.
    pub fn present(&mut self, cv: &Canvas, offset: usize) -> Res<()> {
        let t = cv.tile;
        if t.is_empty() {
            return Ok(());
        }
        if let Some(s) = &self.shm {
            self.c.shm_put_image(
                self.win,
                self.gc,
                t.w as u16,
                t.h as u16,
                0,
                0,
                t.w as u16,
                t.h as u16,
                t.x as i16,
                t.y as i16,
                self.depth,
                ImageFormat::Z_PIXMAP.into(),
                false,
                s.seg,
                (offset * 4) as u32,
            )?;
        } else {
            let bytes = cv.as_bytes();
            let max = self.c.maximum_request_bytes().saturating_sub(64).max(4096);
            let row_bytes = t.w as usize * 4;
            let rows = (max / row_bytes).max(1);
            let mut y = 0usize;
            while y < t.h as usize {
                let n = rows.min(t.h as usize - y);
                self.c.put_image(
                    ImageFormat::Z_PIXMAP,
                    self.win,
                    self.gc,
                    t.w as u16,
                    n as u16,
                    t.x as i16,
                    (t.y + y as i32) as i16,
                    0,
                    self.depth,
                    &bytes[y * row_bytes..(y + n) * row_bytes],
                )?;
                y += n;
            }
        }
        Ok(())
    }

    /// Split `r` into tiles that fit the canvas.
    pub fn tiles(r: Rect, out: &mut Vec<Rect>) {
        if r.is_empty() {
            return;
        }
        let rows = (TILE_PIXELS as i32 / r.w.max(1)).max(1);
        let mut y = r.y;
        while y < r.bottom() {
            let h = rows.min(r.bottom() - y);
            out.push(Rect::new(r.x, y, r.w.min(TILE_PIXELS as i32), h));
            y += h;
        }
    }
}

fn make_cursors(c: &RustConnection) -> [u32; 2] {
    let mut out = [0u32; 2];
    let Ok(font) = c.generate_id() else {
        return out;
    };
    if c.open_font(font, b"cursor").is_err() {
        return out;
    }
    // left_ptr and hand2 of the cursor font.
    for (i, glyph) in [68u16, 60].into_iter().enumerate() {
        if let Ok(cur) = c.generate_id() {
            if c.create_glyph_cursor(
                cur,
                font,
                font,
                glyph,
                glyph + 1,
                0,
                0,
                0,
                0xffff,
                0xffff,
                0xffff,
            )
            .is_ok()
            {
                out[i] = cur;
            }
        }
    }
    let _ = c.close_font(font);
    out
}

impl Drop for X {
    fn drop(&mut self) {
        if let Some(s) = self.shm.take() {
            let _ = self.c.shm_detach(s.seg);
            let _ = self.c.flush();
            unsafe { libc::shmdt(s.addr as *const libc::c_void) };
        }
        let _ = self.c.flush();
    }
}

/// The screen size of a connection.
pub fn screen_of(c: &RustConnection, screen: usize) -> (i32, i32) {
    let s = &c.setup().roots[screen];
    (s.width_in_pixels as i32, s.height_in_pixels as i32)
}

/// When another instance owns the selection `claim`, ask it to show
/// `what` (a section, or "" for nothing in particular), raise its window
/// and return true.
pub fn raise_running(
    c: &RustConnection,
    screen: usize,
    claim: &str,
    prop: &str,
    what: &str,
) -> bool {
    let root = c.setup().roots[screen].root;
    let atom = |n: &str| -> Option<Atom> {
        Some(c.intern_atom(false, n.as_bytes()).ok()?.reply().ok()?.atom)
    };
    let (Some(sel), Some(p), Some(active)) = (atom(claim), atom(prop), atom("_NET_ACTIVE_WINDOW"))
    else {
        return false;
    };
    let Some(owner) = c
        .get_selection_owner(sel)
        .ok()
        .and_then(|ck| ck.reply().ok())
        .map(|r| r.owner)
        .filter(|&w| w != x11rb::NONE)
    else {
        return false;
    };
    let text = if what.is_empty() { "raise" } else { what };
    let _ = c.change_property8(
        PropMode::REPLACE,
        owner,
        p,
        AtomEnum::STRING,
        text.as_bytes(),
    );
    let _ = c.map_window(owner);
    let ev = ClientMessageEvent::new(32, owner, active, [2u32, x11rb::CURRENT_TIME, 0, 0, 0]);
    let _ = c.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        ev,
    );
    let _ = c.configure_window(
        owner,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    );
    let _ = c.flush();
    let _ = c.get_input_focus().ok().and_then(|ck| ck.reply().ok());
    true
}
