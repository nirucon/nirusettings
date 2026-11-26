//! The window's state: the sections, the rows of the open view, the
//! selection, the editors, the pending writes and the undo history, and
//! what keys and the mouse do with them.
use crate::gui::canvas::Rect;
use crate::gui::shell::{trace_on, App, Frame, KeyEv, Mouse, Redraw};
use crate::gui::text::{Size, Text};
use crate::gui::theme::Theme;
use crate::gui::x::Pointer;
use crate::worker::{self, After, Req, Resp, Worker};
use nirusettings::cli;
use nirusettings::schema::{self, Kind, SECTIONS};
use nirusettings::sources::{self, Extra, Key, Nirux};
use nirusettings::store::{self, Change, Current, Source, Store};
use nirusettings::validate;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use toml::Value;

/// How long changes wait before they are written (so a held + key is one
/// write, not twenty).
const SETTLE: Duration = Duration::from_millis(350);
const BLINK: Duration = Duration::from_millis(530);

pub fn trace(s: &str) {
    if trace_on() {
        eprintln!("trace|{s}");
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    NiruxUpdate,
    ExtrasUpdate,
    NiruxStatus,
    Refresh,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Head(String),
    Set(usize),
    Key(usize),
    Extra(usize),
    Info(String, String),
    Act(Act, String),
    Note(String),
}

impl Row {
    pub fn selectable(&self) -> bool {
        matches!(
            self,
            Row::Set(_) | Row::Key(_) | Row::Extra(_) | Row::Act(..)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Side,
    Main,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Warn,
    Err,
}

pub struct Editor {
    pub entry: usize,
    pub text: String,
    /// Cursor position in characters.
    pub cur: usize,
    pub error: Option<String>,
}

pub struct Picker {
    pub entry: usize,
    pub items: Vec<(Value, String, String)>,
    pub sel: usize,
    pub top: usize,
}

pub struct ListEd {
    pub entry: usize,
    /// The items that are on, in order.
    pub on: Vec<String>,
    pub sel: usize,
    pub top: usize,
}

impl ListEd {
    /// Every row: the items on (in order), then the ones off.
    pub fn rows(&self, all: &[String]) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self.on.iter().map(|s| (s.clone(), true)).collect();
        for a in all {
            if !self.on.contains(a) {
                out.push((a.clone(), false));
            }
        }
        out
    }
}

pub enum ConfirmAct {
    Terminal(String, After),
}

pub struct Confirm {
    pub text: String,
    pub act: ConfirmAct,
}

struct Undo {
    entry: usize,
    /// The value in the file before (None: the key was absent).
    before: Option<Value>,
    job: u64,
}

struct Pending {
    file: usize,
    changes: Vec<Change>,
    apply: Vec<Vec<String>>,
    at: Instant,
    /// The entries changed, for the status line.
    entries: Vec<usize>,
}

/// Where things are on screen (window coordinates).
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    pub header: Rect,
    pub search: Rect,
    pub side: Rect,
    pub main: Rect,
    pub foot: Rect,
    pub rows: Rect,
    pub row_h: i32,
    pub side_row_h: i32,
    pub value_x: i32,
    pub visible: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub bg: u32,
    pub panel: u32,
    pub field: u32,
    pub fg: u32,
    pub muted: u32,
    pub dim: u32,
    pub line: u32,
    pub border: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub sel: u32,
    pub ok: u32,
    pub warn: u32,
    pub err: u32,
    pub light: bool,
}

pub struct SettingsApp {
    pub store: Store,
    pub themes: Vec<String>,
    pub section: usize,
    pub focus: Focus,
    pub rows: Vec<Row>,
    pub sel: usize,
    pub top: usize,
    pub side_sel: usize,
    pub editor: Option<Editor>,
    pub picker: Option<Picker>,
    pub listed: Option<ListEd>,
    pub confirm: Option<Confirm>,
    pub query: String,
    pub hover: Option<usize>,
    pub hover_side: Option<usize>,
    pub mouse: (i32, i32),
    pub status: (String, Tone),
    undo: Vec<Undo>,
    pending: Vec<Pending>,
    next_job: u64,
    inflight: HashMap<u64, Vec<usize>>,
    worker: Worker,
    pub keys: Option<(Vec<Key>, String)>,
    pub extras: Option<(Vec<Extra>, String)>,
    pub nirux: Option<Nirux>,
    pub versions: HashMap<String, Option<String>>,
    pub warnings: Vec<String>,
    /// Theme colours for the theme swatches, by name.
    pub swatches: HashMap<String, Vec<u32>>,
    pub l: Layout,
    pub c: Colors,
    pub body: Size,
    pub small: Size,
    theme_key: (String, u32, u32, f32),
    pub blink: bool,
    blink_at: Option<Instant>,
    pub has_focus: bool,
    quit: bool,
    title: String,
    pub busy: usize,
    loaded_keys: bool,
    loaded_extras: bool,
    loaded_nirux: bool,
}

fn as_string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

impl SettingsApp {
    pub fn new(section: Option<usize>) -> SettingsApp {
        let (schemas, warnings) = schema::load_all();
        for w in &warnings {
            eprintln!("nirusettings: {w}");
        }
        let store = Store::new(schemas);
        let worker = worker::start(crate::gui::shell::waker());
        let mut bins: Vec<String> = Vec::new();
        for s in &store.schemas {
            if !s.binary.is_empty() && !bins.contains(&s.binary) {
                bins.push(s.binary.clone());
            }
        }
        let _ = worker.tx.send(Req::Versions(bins));
        let mut app = SettingsApp {
            store,
            themes: sources::themes(),
            section: section.unwrap_or(0),
            focus: Focus::Side,
            rows: Vec::new(),
            sel: 0,
            top: 0,
            side_sel: section.unwrap_or(0),
            editor: None,
            picker: None,
            listed: None,
            confirm: None,
            query: String::new(),
            hover: None,
            hover_side: None,
            mouse: (-1, -1),
            status: (
                "Arrows move, Return edits, / searches, ? shows every key.".into(),
                Tone::Info,
            ),
            undo: Vec::new(),
            pending: Vec::new(),
            next_job: 1,
            inflight: HashMap::new(),
            worker,
            keys: None,
            extras: None,
            nirux: None,
            versions: HashMap::new(),
            warnings,
            swatches: HashMap::new(),
            l: Layout::default(),
            c: Colors {
                bg: 0,
                panel: 0,
                field: 0,
                fg: 0xffffff,
                muted: 0x888888,
                dim: 0x555555,
                line: 0x333333,
                border: 0x444444,
                accent: 0x88c0d0,
                on_accent: 0,
                sel: 0x222222,
                ok: 0x8fbf7f,
                warn: 0xd7b36a,
                err: 0xd77a7a,
                light: false,
            },
            body: Size {
                px: 14.0,
                ascent: 11,
                height: 15,
            },
            small: Size {
                px: 12.0,
                ascent: 9,
                height: 13,
            },
            theme_key: (String::new(), 0, 0, 0.0),
            blink: true,
            blink_at: None,
            has_focus: true,
            quit: false,
            title: "NIRUSETTINGS".into(),
            busy: 0,
            loaded_keys: false,
            loaded_extras: false,
            loaded_nirux: false,
        };
        if section.is_some() {
            app.focus = Focus::Main;
        }
        if let Some(w) = app.warnings.first().cloned() {
            app.status(format!("Schema problem: {w}"), Tone::Warn);
        }
        app.rebuild();
        app
    }

    /// The folders to watch for changes by others.
    pub fn watch_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut out: Vec<std::path::PathBuf> = Vec::new();
        for f in &self.store.files {
            if let Some(d) = f.path.parent() {
                if !out.iter().any(|o| o == d) {
                    out.push(d.to_path_buf());
                }
            }
        }
        out
    }

    pub fn dynamic(&self, e: usize) -> &[String] {
        match self.store.entries[e].setting.choices_from.as_deref() {
            Some("themes") => &self.themes,
            _ => &[],
        }
    }

    pub fn current(&self, e: usize) -> Current {
        self.store.current(e, self.dynamic(e))
    }

    pub fn modified(&self, e: usize) -> bool {
        let cur = self.current(e);
        self.store.modified(e, &cur)
    }

    /// Entries of a section.
    pub fn section_entries(&self, sec: usize) -> Vec<usize> {
        let name = SECTIONS[sec];
        (0..self.store.entries.len())
            .filter(|&e| self.store.entries[e].setting.section == name)
            .collect()
    }

    pub fn section_modified(&self, sec: usize) -> usize {
        self.section_entries(sec)
            .into_iter()
            .filter(|&e| self.modified(e))
            .count()
    }

    pub fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    fn matches(&self, e: usize, words: &[String]) -> bool {
        let st = &self.store.entries[e].setting;
        let s = &self.store.schemas[self.store.entries[e].schema];
        let hay = format!(
            "{} {} {} {} {} {} {}",
            st.label, st.key, st.help, st.section, st.group, s.name, s.component
        )
        .to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    }

    /// Build the rows of the open view (or of the search).
    pub fn rebuild(&mut self) {
        let keep = self.rows.get(self.sel).cloned();
        let mut rows = Vec::new();
        if self.searching() {
            let words: Vec<String> = self
                .query
                .to_lowercase()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            let mut last = String::new();
            for (sec, name) in SECTIONS.iter().enumerate().take(6) {
                for e in self.section_entries(sec) {
                    if self.matches(e, &words) {
                        if last != *name {
                            last = name.to_string();
                            rows.push(Row::Head(last.clone()));
                        }
                        rows.push(Row::Set(e));
                    }
                }
            }
            if let Some((keys, _)) = &self.keys {
                let mut first = true;
                for (i, k) in keys.iter().enumerate() {
                    let hay = format!("{} {} {} {}", k.label, k.keys, k.description, k.group)
                        .to_lowercase();
                    if words.iter().all(|w| hay.contains(w.as_str())) {
                        if first {
                            rows.push(Row::Head("Key bindings".into()));
                            first = false;
                        }
                        rows.push(Row::Key(i));
                    }
                }
            }
            if rows.is_empty() {
                rows.push(Row::Note(format!("Nothing matches \"{}\".", self.query)));
            }
            trace(&format!(
                "search|{}|{}",
                self.query,
                rows.iter().filter(|r| r.selectable()).count()
            ));
        } else {
            match SECTIONS[self.section] {
                "Key bindings" => self.key_rows(&mut rows),
                "Extras" => self.extra_rows(&mut rows),
                "Updates" => self.update_rows(&mut rows),
                _ => {
                    let entries = self.section_entries(self.section);
                    let mut group = String::new();
                    let mut schema_name = String::new();
                    for e in entries {
                        let st = &self.store.entries[e].setting;
                        let sname = self.store.schema_of(e).name.clone();
                        if st.group != group || sname != schema_name {
                            group = st.group.clone();
                            schema_name = sname;
                            rows.push(Row::Head(group.clone()));
                        }
                        rows.push(Row::Set(e));
                    }
                }
            }
        }
        if self.rows != rows {
            self.hover = None;
        }
        self.rows = rows;
        self.sel = keep
            .filter(Row::selectable)
            .and_then(|k| self.rows.iter().position(|r| *r == k))
            .or_else(|| self.rows.iter().position(Row::selectable))
            .unwrap_or(0);
        self.top = self.top.min(self.rows.len().saturating_sub(1));
        self.scroll_to_sel();
    }

    fn key_rows(&mut self, rows: &mut Vec<Row>) {
        if !self.loaded_keys {
            self.loaded_keys = true;
            let cfg = self
                .store
                .files
                .iter()
                .find(|f| f.rel == "niruwm/config.toml")
                .map(|f| f.value.clone())
                .unwrap_or(Value::Table(Default::default()));
            self.busy += 1;
            let _ = self.worker.tx.send(Req::Keys(cfg));
        }
        match &self.keys {
            None => rows.push(Row::Note("Asking niructl for the key bindings...".into())),
            Some((keys, from)) => {
                rows.push(Row::Note(format!(
                    "{} bindings, from {from}. Read only: change them in [[keys]] of niruwm/config.toml.",
                    keys.len()
                )));
                let mut order: Vec<usize> = (0..keys.len()).collect();
                let rank = |g: &str| {
                    [
                        "Programs",
                        "Launcher",
                        "Windows",
                        "Layouts",
                        "Workspaces",
                        "Session",
                    ]
                    .iter()
                    .position(|x| *x == g)
                    .unwrap_or(9)
                };
                order.sort_by_key(|&i| (rank(&keys[i].group), keys[i].group.clone()));
                let mut group = String::new();
                for i in order {
                    if keys[i].group != group {
                        group = keys[i].group.clone();
                        rows.push(Row::Head(if group.is_empty() {
                            "Other".into()
                        } else {
                            group.clone()
                        }));
                    }
                    rows.push(Row::Key(i));
                }
            }
        }
    }

    fn extra_rows(&mut self, rows: &mut Vec<Row>) {
        if !self.loaded_extras {
            self.loaded_extras = true;
            self.busy += 1;
            let _ = self.worker.tx.send(Req::Extras);
        }
        match &self.extras {
            None => rows.push(Row::Note("Asking nirux for the extras...".into())),
            Some((list, from)) => {
                rows.push(Row::Note(format!(
                    "{} extras, state from {from}. Return installs or updates, Delete removes, in a terminal.",
                    list.len()
                )));
                rows.push(Row::Act(
                    Act::ExtrasUpdate,
                    "Update every selected extra (nirux extras update)".into(),
                ));
                rows.push(Row::Act(Act::Refresh, "Ask nirux again".into()));
                rows.push(Row::Head("Extras".into()));
                for i in 0..list.len() {
                    rows.push(Row::Extra(i));
                }
            }
        }
    }

    fn update_rows(&mut self, rows: &mut Vec<Row>) {
        if !self.loaded_nirux {
            self.loaded_nirux = true;
            self.busy += 1;
            let _ = self.worker.tx.send(Req::Nirux);
        }
        rows.push(Row::Head("NIRUX".into()));
        match &self.nirux {
            None => rows.push(Row::Note("Asking for the versions...".into())),
            Some(n) => {
                rows.push(Row::Info(
                    "Installed".into(),
                    n.installed
                        .clone()
                        .map(|v| format!("NIRUX {v}"))
                        .unwrap_or_else(|| "nirux is not installed".into()),
                ));
                if let Some(l) = &n.latest {
                    let newer = n.installed.as_deref().is_some_and(|i| sources::older(i, l));
                    rows.push(Row::Info(
                        "Newest release".into(),
                        if newer {
                            format!("{l}: an update is ready")
                        } else {
                            format!("{l} (from the bar's daily check)")
                        },
                    ));
                }
                rows.push(Row::Act(
                    Act::NiruxUpdate,
                    "Update NIRUX now (nirux update, in a terminal)".into(),
                ));
                rows.push(Row::Act(
                    Act::ExtrasUpdate,
                    "Update the extras (nirux extras update)".into(),
                ));
                rows.push(Row::Act(
                    Act::NiruxStatus,
                    "What is installed and running (nirux status)".into(),
                ));
                rows.push(Row::Head("Components".into()));
                for (c, v) in &n.components {
                    rows.push(Row::Info(
                        c.to_uppercase(),
                        v.clone().unwrap_or_else(|| "not installed".into()),
                    ));
                }
            }
        }
    }

    pub fn scroll_to_sel(&mut self) {
        let vis = self.l.visible.max(1);
        if self.sel < self.top {
            self.top = self.sel;
            // Keep the heading above the first row in view.
            if self.top > 0 && matches!(self.rows[self.top - 1], Row::Head(_) | Row::Note(_)) {
                self.top -= 1;
            }
        } else if self.sel >= self.top + vis {
            self.top = self.sel + 1 - vis;
        }
        let max_top = self.rows.len().saturating_sub(vis);
        self.top = self.top.min(max_top);
    }

    fn move_sel(&mut self, d: i32) {
        if self.rows.is_empty() {
            return;
        }
        let mut i = self.sel as i32;
        let n = self.rows.len() as i32;
        loop {
            i += d;
            if i < 0 || i >= n {
                break;
            }
            if self.rows[i as usize].selectable() {
                self.sel = i as usize;
                break;
            }
        }
        self.scroll_to_sel();
        self.trace_sel();
    }

    fn page(&mut self, d: i32) {
        let vis = self.l.visible.max(1) as i32 - 1;
        let target = (self.sel as i32 + d * vis).clamp(0, self.rows.len() as i32 - 1) as usize;
        let mut best = self.sel;
        for (i, r) in self.rows.iter().enumerate() {
            if r.selectable()
                && (i as i32 - target as i32).abs() < (best as i32 - target as i32).abs()
            {
                best = i;
            }
        }
        self.sel = best;
        self.scroll_to_sel();
        self.trace_sel();
    }

    fn trace_sel(&self) {
        if let Some(Row::Set(e)) = self.rows.get(self.sel) {
            trace(&format!("select|{}", cli::id(&self.store, *e)));
        }
    }

    fn open_section(&mut self, s: usize) {
        if s >= SECTIONS.len() {
            return;
        }
        self.section = s;
        self.side_sel = s;
        self.query.clear();
        self.top = 0;
        self.rows.clear();
        self.rebuild();
        trace(&format!("section|{}", SECTIONS[s]));
    }

    pub fn status(&mut self, s: impl Into<String>, t: Tone) {
        let s = s.into();
        trace(&format!("status|{t:?}|{s}"));
        self.status = (s, t);
    }

    /// The installed version of the schema's program, when known.
    pub fn version_of(&self, e: usize) -> Option<&str> {
        let b = &self.store.schema_of(e).binary;
        self.versions.get(b).and_then(|v| v.as_deref())
    }

    /// "needs NIRULOCK 0.30 (0.29.0 installed)" for a new key.
    pub fn since_note(&self, e: usize) -> Option<String> {
        let st = &self.store.entries[e].setting;
        let since = st.since.as_deref()?;
        let name = &self.store.schema_of(e).name;
        match self.version_of(e) {
            Some(v) if !sources::older(v, since) => None,
            Some(v) => Some(format!("needs {name} {since} or newer ({v} installed)")),
            None => Some(format!("needs {name} {since} or newer")),
        }
    }

    // ── Changing values ────────────────────────────────────────────────

    /// Set entry `e` to `v` (None removes the key): check, show at once,
    /// write after a moment.
    pub fn commit(&mut self, e: usize, v: Option<Value>, record: bool) -> bool {
        let st = self.store.entries[e].setting.clone();
        if st.read_only {
            self.status(
                format!("{} is shown, not changed, here.", st.label),
                Tone::Warn,
            );
            return false;
        }
        if let Some(v) = &v {
            if let Err(err) = validate::value(&st, v, self.dynamic(e)) {
                self.status(
                    format!("{}: {err}. Nothing was changed.", st.label),
                    Tone::Err,
                );
                return false;
            }
        }
        let cur = self.current(e);
        let before = if cur.source == Source::File {
            cur.value.clone()
        } else {
            None
        };
        let unchanged = match (&before, &v) {
            (Some(a), Some(b)) => store::same(a, b),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return false;
        }
        let fi = self.store.entries[e].file;
        let changes = self.store.changes_for(e, v.clone());
        if let Err(err) = self.store.apply_local(fi, &changes) {
            self.status(err, Tone::Err);
            return false;
        }
        if record {
            self.undo.push(Undo {
                entry: e,
                before,
                job: 0,
            });
        }
        let apply = cli::apply_mode(&self.store, e).run.clone();
        let now = Instant::now();
        match self.pending.iter_mut().find(|p| p.file == fi) {
            Some(p) => {
                for c in changes {
                    p.changes.retain(|o| o.names != c.names);
                    p.changes.push(c);
                }
                if !apply.is_empty() && !p.apply.contains(&apply) {
                    p.apply.push(apply);
                }
                if !p.entries.contains(&e) {
                    p.entries.push(e);
                }
                p.at = now + SETTLE;
            }
            None => self.pending.push(Pending {
                file: fi,
                changes,
                apply: if apply.is_empty() {
                    vec![]
                } else {
                    vec![apply]
                },
                at: now + SETTLE,
                entries: vec![e],
            }),
        }
        let shown = v
            .as_ref()
            .map(|v| short_value(&st, v))
            .unwrap_or_else(|| "unset".into());
        self.status(format!("{} = {shown} · saving...", st.label), Tone::Info);
        if self.listed.as_ref().is_some_and(|ed| ed.entry == e) {
            let on = as_string_list(self.current(e).value.as_ref());
            if let Some(ed) = &mut self.listed {
                ed.on = on;
            }
        }
        true
    }

    /// Send the writes whose moment has come (all of them with `all`).
    fn flush(&mut self, all: bool) {
        let now = Instant::now();
        let mut i = 0;
        while i < self.pending.len() {
            if all || self.pending[i].at <= now {
                let p = self.pending.remove(i);
                let id = self.next_job;
                self.next_job += 1;
                let job = cli::job_for_file(&self.store, p.file, p.changes, &p.apply, id);
                for u in self.undo.iter_mut().filter(|u| u.job == 0) {
                    if self.store.entries[u.entry].file == p.file {
                        u.job = id;
                    }
                }
                self.inflight.insert(id, p.entries);
                self.busy += 1;
                let _ = self.worker.tx.send(Req::Write(job));
            } else {
                i += 1;
            }
        }
    }

    fn written(&mut self, d: nirusettings::writer::Done) {
        self.busy = self.busy.saturating_sub(1);
        let entries = self.inflight.remove(&d.id).unwrap_or_default();
        let label = entries
            .iter()
            .map(|&e| self.store.entries[e].setting.label.clone())
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(err) = d.error {
            // Nothing was written: show the file as it is.
            self.store.reload(d.file);
            self.undo.retain(|u| u.job != d.id);
            self.refresh_editors();
            for &e in &entries {
                trace(&format!("refused|{}|{err}", cli::id(&self.store, e)));
            }
            self.status(format!("{label}: not saved. {err}"), Tone::Err);
            return;
        }
        // Other changes may have been made meanwhile; read the file back.
        let pending_here = self.pending.iter().any(|p| p.file == d.file);
        if !pending_here {
            self.store.reload(d.file);
        }
        for &e in &entries {
            let cur = self.current(e);
            trace(&format!(
                "saved|{}|{}",
                cli::id(&self.store, e),
                cur.value.as_ref().map(schema::plain).unwrap_or_default()
            ));
        }
        let mut msg = format!("Saved {label}");
        let mut tone = Tone::Ok;
        let says: Vec<String> = entries
            .iter()
            .map(|&e| cli::apply_mode(&self.store, e).says.clone())
            .fold(Vec::new(), |mut v, s| {
                if !v.contains(&s) {
                    v.push(s);
                }
                v
            });
        if !d.apply_errors.is_empty() {
            msg.push_str(&format!(
                " · not applied yet ({}); it is used at the next start",
                d.apply_errors.join("; ")
            ));
            tone = Tone::Warn;
        } else if !says.is_empty() {
            msg.push_str(&format!(" · {}", says.join(", ")));
        }
        for &e in &entries {
            if let Some(n) = self.since_note(e) {
                msg.push_str(&format!(" · {n}"));
                tone = Tone::Warn;
            }
        }
        if !d.unchecked.is_empty() && d.checked.is_empty() {
            msg.push_str(&format!(
                " · checked here only ({} not installed)",
                d.unchecked.join(", ")
            ));
        }
        if let Some(b) = d.backup {
            msg.push_str(&format!(
                " · old file kept as {}",
                b.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            ));
        }
        self.status(msg, tone);
        self.refresh_editors();
    }

    fn refresh_editors(&mut self) {
        if let Some(ed) = &self.listed {
            let e = ed.entry;
            let on = as_string_list(self.current(e).value.as_ref());
            if let Some(ed) = &mut self.listed {
                ed.on = on;
            }
        }
    }

    pub fn do_undo(&mut self) {
        let Some(u) = self.undo.pop() else {
            self.status("Nothing to undo.", Tone::Info);
            return;
        };
        let label = self.store.entries[u.entry].setting.label.clone();
        trace(&format!("undo|{}", cli::id(&self.store, u.entry)));
        self.commit(u.entry, u.before, false);
        self.status(format!("Undone: {label} · saving..."), Tone::Info);
    }

    pub fn reset(&mut self, e: usize) {
        let st = self.store.entries[e].setting.clone();
        if st.no_reset {
            self.status(
                format!(
                    "{}: NIRUX sets this one; type the value you want with Return.",
                    st.label
                ),
                Tone::Warn,
            );
            return;
        }
        let cur = self.current(e);
        if !self.store.modified(e, &cur) && cur.source != Source::File {
            self.status(format!("{} is already the default.", st.label), Tone::Info);
            return;
        }
        // A key in the file keeps its place and comments: it gets the
        // default value. An optional key without a default goes.
        let v = st.default.clone();
        if cur.source == Source::File
            && v.as_ref()
                .zip(cur.value.as_ref())
                .is_some_and(|(a, b)| store::same(a, b))
        {
            self.status(format!("{} is already the default.", st.label), Tone::Info);
            return;
        }
        trace(&format!("reset|{}", cli::id(&self.store, e)));
        self.commit(e, v, true);
    }

    /// Left/right, +/-: the next or previous value.
    fn step(&mut self, e: usize, d: i32, big: bool) {
        let st = self.store.entries[e].setting.clone();
        let cur = self.current(e);
        match st.kind {
            Kind::Bool => {
                let b = cur.value.as_ref().and_then(Value::as_bool).unwrap_or(false);
                self.commit(e, Some(Value::Boolean(!b)), true);
            }
            Kind::Int => {
                let n = cur.value.as_ref().and_then(Value::as_integer).unwrap_or(0);
                let mut s = st.step.unwrap_or(1.0).max(1.0) as i64;
                if big {
                    s *= 10;
                }
                let mut v = n + s * d as i64;
                if let Some(lo) = st.min {
                    v = v.max(lo as i64);
                }
                if let Some(hi) = st.max {
                    v = v.min(hi as i64);
                }
                if v != n {
                    self.commit(e, Some(Value::Integer(v)), true);
                } else {
                    self.status(format!("{}: {}", st.label, limit_text(&st, d)), Tone::Info);
                }
            }
            Kind::Float => {
                let n = cur
                    .value
                    .as_ref()
                    .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                    .unwrap_or(0.0);
                let mut s = st.step.unwrap_or(0.1);
                if big {
                    s *= 10.0;
                }
                let mut v = ((n + s * d as f64) / s).round() * s;
                v = (v * 1000.0).round() / 1000.0;
                if let Some(lo) = st.min {
                    v = v.max(lo);
                }
                if let Some(hi) = st.max {
                    v = v.min(hi);
                }
                if (v - n).abs() > 1e-9 {
                    self.commit(e, Some(Value::Float(v)), true);
                } else {
                    self.status(format!("{}: {}", st.label, limit_text(&st, d)), Tone::Info);
                }
            }
            Kind::Enum => {
                let choices = self.store.choices(e, self.dynamic(e));
                if choices.is_empty() {
                    return;
                }
                let n = choices.len() as i32;
                let at = cur
                    .value
                    .as_ref()
                    .and_then(|v| choices.iter().position(|(c, _)| store::same(c, v)));
                let next = match (at, st.optional) {
                    // Optional: the cycle passes through "unset".
                    (None, true) if d > 0 => Some(0),
                    (None, true) => Some(n - 1),
                    (None, false) => Some(0),
                    (Some(i), true) if (i as i32 + d) < 0 || i as i32 + d >= n => None,
                    (Some(i), _) => Some((i as i32 + d).rem_euclid(n)),
                };
                let v = next.map(|i| choices[i as usize].0.clone());
                self.commit(e, v, true);
            }
            _ => self.open_editor(e),
        }
    }

    /// Return on a setting: the editor that fits it.
    fn activate(&mut self, e: usize) {
        let st = self.store.entries[e].setting.clone();
        if st.read_only {
            self.status(st.help.clone(), Tone::Info);
            return;
        }
        match st.kind {
            Kind::Bool => self.step(e, 1, false),
            Kind::Enum => {
                let items: Vec<(Value, String, String)> = {
                    let mut v: Vec<(Value, String, String)> = Vec::new();
                    if st.optional {
                        v.push((
                            Value::String(String::new()),
                            "unset".into(),
                            "remove the key: the component's own default".into(),
                        ));
                    }
                    if st.choices.is_empty() {
                        for (val, label) in self.store.choices(e, self.dynamic(e)) {
                            v.push((val, label, String::new()));
                        }
                    } else {
                        for c in &st.choices {
                            v.push((c.value.clone(), c.label.clone(), c.help.clone()));
                        }
                    }
                    v
                };
                let cur = self.current(e);
                let sel = cur
                    .value
                    .as_ref()
                    .and_then(|v| items.iter().position(|(c, _, _)| store::same(c, v)))
                    .unwrap_or(0);
                self.picker = Some(Picker {
                    entry: e,
                    items,
                    sel,
                    top: 0,
                });
                trace(&format!("picker|{}", cli::id(&self.store, e)));
            }
            Kind::List if st.item_kind() == Kind::Enum && st.unique => {
                let on = as_string_list(self.current(e).value.as_ref());
                self.listed = Some(ListEd {
                    entry: e,
                    on,
                    sel: 0,
                    top: 0,
                });
                trace(&format!("listed|{}", cli::id(&self.store, e)));
            }
            _ => self.open_editor(e),
        }
    }

    fn open_editor(&mut self, e: usize) {
        let st = &self.store.entries[e].setting;
        let cur = self.current(e);
        let text = validate::as_input(st, cur.value.as_ref());
        let cur_pos = text.chars().count();
        self.editor = Some(Editor {
            entry: e,
            text,
            cur: cur_pos,
            error: None,
        });
        self.blink = true;
        self.blink_at = Some(Instant::now() + BLINK);
        trace(&format!("editor|{}", cli::id(&self.store, e)));
    }

    fn editor_key(&mut self, k: &KeyEv) {
        let Some(ed) = &mut self.editor else { return };
        self.blink = true;
        self.blink_at = Some(Instant::now() + BLINK);
        match k.sym {
            0xff1b => {
                self.editor = None;
                self.status("Not changed.", Tone::Info);
            }
            0xff0d | 0xff8d => {
                let e = ed.entry;
                let text = ed.text.clone();
                let st = self.store.entries[e].setting.clone();
                match validate::parse_input(&st, &text, self.dynamic(e)) {
                    Ok(v) => {
                        self.editor = None;
                        if !self.commit(e, v, true) && self.status.1 != Tone::Err {
                            self.status("Not changed.", Tone::Info);
                        }
                    }
                    Err(err) => {
                        trace(&format!("invalid|{}|{err}", cli::id(&self.store, e)));
                        if let Some(ed) = &mut self.editor {
                            ed.error = Some(err);
                        }
                    }
                }
            }
            0xff08 => {
                if ed.cur > 0 {
                    let b = byte_at(&ed.text, ed.cur - 1);
                    ed.text.remove(b);
                    ed.cur -= 1;
                }
                ed.error = None;
            }
            0xffff => {
                if ed.cur < ed.text.chars().count() {
                    let b = byte_at(&ed.text, ed.cur);
                    ed.text.remove(b);
                }
                ed.error = None;
            }
            0xff51 => ed.cur = ed.cur.saturating_sub(1),
            0xff53 => ed.cur = (ed.cur + 1).min(ed.text.chars().count()),
            0xff50 => ed.cur = 0,
            0xff57 => ed.cur = ed.text.chars().count(),
            _ if k.ctrl && k.ch == Some('u') => {
                ed.text.clear();
                ed.cur = 0;
                ed.error = None;
            }
            _ if k.ctrl && k.ch == Some('a') => ed.cur = 0,
            _ if k.ctrl && k.ch == Some('e') => ed.cur = ed.text.chars().count(),
            _ => {
                if let Some(c) = k.ch {
                    if !k.ctrl && !c.is_control() && ed.text.len() < 4096 {
                        let b = byte_at(&ed.text, ed.cur);
                        ed.text.insert(b, c);
                        ed.cur += 1;
                        ed.error = None;
                    }
                }
            }
        }
    }

    fn picker_key(&mut self, k: &KeyEv) {
        let Some(p) = &mut self.picker else { return };
        let n = p.items.len();
        match k.sym {
            0xff1b | 0xff51 => {
                self.picker = None;
            }
            0xff52 => p.sel = p.sel.saturating_sub(1),
            0xff54 => p.sel = (p.sel + 1).min(n.saturating_sub(1)),
            0xff55 => p.sel = p.sel.saturating_sub(8),
            0xff56 => p.sel = (p.sel + 8).min(n.saturating_sub(1)),
            0xff50 => p.sel = 0,
            0xff57 => p.sel = n.saturating_sub(1),
            0xff0d | 0xff8d | 0x20 | 0xff53 => self.pick(),
            _ => {
                if let Some(c) = k.ch.map(|c| c.to_ascii_lowercase()) {
                    let start = p.sel + 1;
                    if let Some(i) = (0..n)
                        .map(|i| (start + i) % n)
                        .find(|&i| p.items[i].1.to_lowercase().starts_with(c))
                    {
                        p.sel = i;
                    }
                }
            }
        }
    }

    fn pick(&mut self) {
        let Some(p) = self.picker.take() else { return };
        let (v, _, _) = p.items[p.sel].clone();
        let st = &self.store.entries[p.entry].setting;
        let v = if st.optional && v.as_str() == Some("") {
            None
        } else {
            Some(v)
        };
        if !self.commit(p.entry, v, true) && self.status.1 != Tone::Err {
            self.status("Not changed.", Tone::Info);
        }
    }

    fn list_all(&self, e: usize) -> Vec<String> {
        self.store.entries[e]
            .setting
            .choices
            .iter()
            .map(|c| schema::plain(&c.value))
            .collect()
    }

    fn listed_key(&mut self, k: &KeyEv) {
        let Some(ed) = &self.listed else { return };
        let e = ed.entry;
        let all = self.list_all(e);
        let rows = ed.rows(&all);
        let n = rows.len();
        let st = self.store.entries[e].setting.clone();
        let moving = k.shift || k.ctrl;
        match k.sym {
            0xff1b | 0xff09 | 0xff51 => {
                self.listed = None;
            }
            0xff52 | 0xff54 if moving => {
                let ed = self.listed.as_mut().unwrap();
                let up = k.sym == 0xff52;
                let i = ed.sel;
                if !st.ordered || i >= ed.on.len() {
                    return;
                }
                let j = if up { i.checked_sub(1) } else { Some(i + 1) };
                let Some(j) = j.filter(|&j| j < ed.on.len()) else {
                    return;
                };
                let mut on = ed.on.clone();
                on.swap(i, j);
                ed.sel = j;
                let v = Value::Array(on.into_iter().map(Value::String).collect());
                self.commit(e, Some(v), true);
            }
            0xff52 => {
                let ed = self.listed.as_mut().unwrap();
                ed.sel = ed.sel.saturating_sub(1);
            }
            0xff54 => {
                let ed = self.listed.as_mut().unwrap();
                ed.sel = (ed.sel + 1).min(n.saturating_sub(1));
            }
            0xff50 => self.listed.as_mut().unwrap().sel = 0,
            0xff57 => self.listed.as_mut().unwrap().sel = n.saturating_sub(1),
            0x20 | 0xff0d | 0xff8d => self.list_toggle(),
            _ => {}
        }
    }

    fn list_toggle(&mut self) {
        let Some(ed) = &self.listed else { return };
        let e = ed.entry;
        let all = self.list_all(e);
        let rows = ed.rows(&all);
        let Some((name, on)) = rows.get(ed.sel).cloned() else {
            return;
        };
        let mut list = ed.on.clone();
        if on {
            list.retain(|x| *x != name);
        } else {
            list.push(name.clone());
        }
        let v = Value::Array(list.iter().cloned().map(Value::String).collect());
        if self.commit(e, Some(v), true) {
            // Keep the cursor on the same item where it moved to.
            if let Some(ed) = &mut self.listed {
                let rows = ed.rows(&all);
                if let Some(i) = rows.iter().position(|(n, _)| *n == name) {
                    ed.sel = i;
                }
            }
        }
    }

    fn run_act(&mut self, a: &Act) {
        match a {
            Act::NiruxUpdate => self.ask_terminal("nirux update", After::Nirux),
            Act::ExtrasUpdate => self.ask_terminal("nirux extras update", After::Extras),
            Act::NiruxStatus => self.terminal("nirux status", After::Nirux),
            Act::Refresh => {
                self.extras = None;
                self.loaded_extras = false;
                self.rebuild();
            }
        }
    }

    fn ask_terminal(&mut self, script: &str, after: After) {
        self.confirm = Some(Confirm {
            text: format!("Run \"{script}\" in a terminal? Return: yes · Esc: no"),
            act: ConfirmAct::Terminal(script.to_string(), after),
        });
        trace(&format!("confirm|{script}"));
    }

    fn terminal(&mut self, script: &str, after: After) {
        let term = self
            .store
            .entries
            .iter()
            .position(|en| {
                en.setting.key == "terminal" && self.store.schemas[en.schema].component == "niruwm"
            })
            .map(|e| as_string_list(self.current(e).value.as_ref()))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| vec!["x-terminal-emulator".into()]);
        let Some(argv) = sources::terminal_command(&term, script) else {
            return;
        };
        trace(&format!("terminal|{}", argv.join(" ")));
        let _ = self.worker.tx.send(Req::Terminal { argv, after });
        self.status(format!("Opened a terminal: {script}"), Tone::Info);
    }

    fn extra_action(&mut self, i: usize, remove: bool) {
        let Some((list, _)) = &self.extras else {
            return;
        };
        let Some(x) = list.get(i).cloned() else {
            return;
        };
        if remove {
            if x.installed.is_none() {
                self.status(format!("{} is not installed.", x.name), Tone::Info);
                return;
            }
            self.ask_terminal(&format!("nirux extras uninstall {}", x.name), After::Extras);
        } else {
            if x.installed.is_some()
                && x.installed == Some(x.pinned.clone())
                && !x.pinned.is_empty()
            {
                self.status(
                    format!("{} {} is installed and up to date.", x.name, x.pinned),
                    Tone::Ok,
                );
                return;
            }
            self.ask_terminal(&format!("nirux extras install {}", x.name), After::Extras);
        }
    }

    fn search_key(&mut self, k: &KeyEv) {
        match k.sym {
            0xff1b => {
                self.query.clear();
                self.focus = Focus::Main;
                self.rebuild();
            }
            0xff0d | 0xff8d => {
                self.focus = Focus::Main;
                if let Some(Row::Set(e)) = self.rows.get(self.sel).cloned() {
                    self.activate(e);
                }
            }
            0xff52 => self.move_sel(-1),
            0xff54 => self.move_sel(1),
            0xff09 => self.focus = Focus::Main,
            0xff08 => {
                self.query.pop();
                self.rebuild();
            }
            _ if k.ctrl && k.ch == Some('u') => {
                self.query.clear();
                self.rebuild();
            }
            _ => {
                if let Some(c) = k.ch {
                    if !k.ctrl && !c.is_control() && self.query.len() < 64 {
                        self.query.push(c);
                        self.sel = 0;
                        self.rebuild();
                    }
                }
            }
        }
    }

    fn main_key(&mut self, k: &KeyEv) {
        let row = self.rows.get(self.sel).cloned();
        let ch = k.ch;
        match k.sym {
            0xff52 => self.move_sel(-1),
            0xff54 => self.move_sel(1),
            0xff55 => self.page(-1),
            0xff56 => self.page(1),
            0xff50 => {
                self.sel = 0;
                self.move_sel(0);
                if !self.rows.get(self.sel).is_some_and(Row::selectable) {
                    self.move_sel(1);
                }
                self.top = 0;
            }
            0xff57 => {
                self.sel = self.rows.len().saturating_sub(1);
                if !self.rows.get(self.sel).is_some_and(Row::selectable) {
                    self.move_sel(-1);
                }
                self.scroll_to_sel();
            }
            0xff1b | 0xff09 | 0xfe20 => {
                if self.searching() && k.sym == 0xff1b {
                    self.query.clear();
                    self.rebuild();
                }
                self.focus = Focus::Side;
            }
            0xff51 | 0xff53 => {
                let d = if k.sym == 0xff51 { -1 } else { 1 };
                match row {
                    Some(Row::Set(e)) => {
                        let kind = self.store.entries[e].setting.kind;
                        if matches!(kind, Kind::Bool | Kind::Int | Kind::Float | Kind::Enum) {
                            self.step(e, d, k.shift);
                        } else if d < 0 {
                            self.focus = Focus::Side;
                        } else {
                            self.activate(e);
                        }
                    }
                    _ if d < 0 => self.focus = Focus::Side,
                    _ => {}
                }
            }
            0xff0d | 0xff8d => match row {
                Some(Row::Set(e)) => self.activate(e),
                Some(Row::Act(a, _)) => self.run_act(&a),
                Some(Row::Extra(i)) => self.extra_action(i, false),
                Some(Row::Key(i)) => {
                    if let Some((keys, _)) = &self.keys {
                        let k = &keys[i];
                        let msg = format!("{}: {} (read only here)", k.label, k.description);
                        self.status(msg, Tone::Info);
                    }
                }
                _ => {}
            },
            0xffff => match row {
                Some(Row::Set(e)) => self.reset(e),
                Some(Row::Extra(i)) => self.extra_action(i, true),
                _ => {}
            },
            0x20 => match row {
                Some(Row::Set(e)) => {
                    let st = &self.store.entries[e].setting;
                    if st.kind == Kind::Bool {
                        self.step(e, 1, false);
                    } else {
                        self.activate(e);
                    }
                }
                Some(Row::Act(a, _)) => self.run_act(&a),
                _ => {}
            },
            _ => match (ch, row) {
                (Some('+') | Some('='), Some(Row::Set(e))) => self.step(e, 1, false),
                (Some('-') | Some('_'), Some(Row::Set(e))) => self.step(e, -1, false),
                (Some('r'), Some(Row::Set(e))) => self.reset(e),
                (Some('e'), Some(Row::Set(e))) => self.open_editor(e),
                (Some('x'), Some(Row::Extra(i))) => self.extra_action(i, true),
                (Some('i'), Some(Row::Extra(i))) => self.extra_action(i, false),
                _ => {}
            },
        }
    }

    fn side_key(&mut self, k: &KeyEv) {
        match k.sym {
            0xff52 => {
                if self.side_sel > 0 {
                    self.open_section(self.side_sel - 1);
                }
            }
            0xff54 => {
                if self.side_sel + 1 < SECTIONS.len() {
                    self.open_section(self.side_sel + 1);
                }
            }
            0xff50 => self.open_section(0),
            0xff57 => self.open_section(SECTIONS.len() - 1),
            0xff53 | 0xff0d | 0xff8d | 0xff09 | 0x20 => {
                self.focus = Focus::Main;
                if !self.rows.get(self.sel).is_some_and(Row::selectable) {
                    self.move_sel(1);
                }
                self.trace_sel();
            }
            _ => {}
        }
    }

    /// The help shown for the selected or hovered row.
    pub fn help_lines(&self) -> (String, String) {
        let row = self
            .hover
            .and_then(|h| self.rows.get(h))
            .or_else(|| self.rows.get(self.sel));
        match row {
            Some(Row::Set(e)) => {
                let e = *e;
                let st = &self.store.entries[e].setting;
                let mut help = st.help.clone();
                if let Some(p) = &self.picker {
                    if p.entry == e && !p.items[p.sel].2.is_empty() {
                        help = format!("{}: {}", p.items[p.sel].1, p.items[p.sel].2);
                    }
                }
                let cur = self.current(e);
                let mode = cli::apply_mode(&self.store, e);
                let f = self.store.file_of(e);
                let src = match cur.source {
                    Source::File => format!(
                        "set in {} as {}",
                        f.rel,
                        cur.name.as_deref().unwrap_or(&st.key)
                    ),
                    Source::Default => format!("default (not in {})", f.rel),
                    Source::Unset => format!("unset (not in {})", f.rel),
                };
                let mut info = format!(
                    "{} · {src} · {}",
                    self.store.schema_of(e).name,
                    if mode.live {
                        "applies live".to_string()
                    } else {
                        mode.says.clone()
                    }
                );
                if let Some(d) = &st.default {
                    info.push_str(&format!(" · default {}", short_value(st, d)));
                }
                if let Some(n) = self.since_note(e) {
                    info.push_str(&format!(" · {n}"));
                }
                if let Some(bad) = &cur.invalid {
                    info = format!("The file's value {bad}. {info}");
                }
                if let Some(err) = &f.error {
                    info = err.clone();
                }
                (help, info)
            }
            Some(Row::Key(i)) => match &self.keys {
                Some((k, _)) => (
                    format!("{}: {}", k[*i].label, k[*i].description),
                    format!("{} · {} · read only", k[*i].group, k[*i].source),
                ),
                None => (String::new(), String::new()),
            },
            Some(Row::Extra(i)) => match &self.extras {
                Some((x, _)) => {
                    let x = &x[*i];
                    (
                        x.description.clone(),
                        format!(
                            "pinned {} · installed {} · {} · Return installs or updates, Delete removes",
                            if x.pinned.is_empty() { "?" } else { &x.pinned },
                            x.installed.as_deref().unwrap_or("no"),
                            if x.status.is_empty() { "-" } else { &x.status }
                        ),
                    )
                }
                None => (String::new(), String::new()),
            },
            Some(Row::Act(_, label)) => (label.clone(), "Return runs it in your terminal.".into()),
            _ => (String::new(), String::new()),
        }
    }

    // ── Layout ─────────────────────────────────────────────────────────

    fn layout(&mut self, w: i32, h: i32) {
        let body_h = self.body.height;
        let header_h = 44;
        let foot_h = body_h * 2 + self.small.height + 22;
        let side_w = (w / 5).clamp(176, 240);
        let gap = 8;
        let l = &mut self.l;
        l.w = w;
        l.h = h;
        l.header = Rect::new(0, 0, w, header_h);
        let sw = (w / 3).clamp(180, 340);
        l.search = Rect::new(w - sw - 14, 10, sw, 24);
        l.side = Rect::new(
            gap + 4,
            header_h + gap,
            side_w,
            h - header_h - foot_h - 2 * gap,
        );
        l.main = Rect::new(
            l.side.right() + gap,
            header_h + gap,
            w - l.side.right() - 2 * gap - 4,
            h - header_h - foot_h - 2 * gap,
        );
        l.foot = Rect::new(gap + 4, h - foot_h - 2, w - 2 * gap - 8, foot_h - 4);
        l.row_h = (body_h + 9).max(20);
        l.side_row_h = (body_h + 13).max(24);
        l.rows = Rect::new(l.main.x + 2, l.main.y + 26, l.main.w - 4, l.main.h - 30);
        l.visible = (l.rows.h / l.row_h).max(1) as usize;
        l.value_x = l.main.x + (l.main.w * 47 / 100).max(200);
    }

    /// Which main row is at (x, y).
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let r = self.l.rows;
        if !r.contains(x, y) {
            return None;
        }
        let i = self.top + ((y - r.y) / self.l.row_h) as usize;
        (i < self.rows.len() && i < self.top + self.l.visible).then_some(i)
    }

    pub fn side_at(&self, x: i32, y: i32) -> Option<usize> {
        let r = self.l.side;
        if !r.contains(x, y) || y < r.y + 26 {
            return None;
        }
        let i = ((y - r.y - 26) / self.l.side_row_h) as usize;
        (i < SECTIONS.len()).then_some(i)
    }

    /// The picker / list editor box.
    pub fn overlay_rect(&self, n: usize) -> Rect {
        let m = self.l.main;
        let rh = self.l.row_h;
        let h = ((n as i32) * rh + 58).min(m.h - 24);
        let w = (m.w * 2 / 3).clamp(300, m.w - 24);
        Rect::new(m.x + (m.w - w) / 2, m.y + (m.h - h) / 2, w, h)
    }

    pub fn overlay_visible(&self, n: usize) -> usize {
        let r = self.overlay_rect(n);
        (((r.h - 58) / self.l.row_h).max(1) as usize).min(n)
    }

    fn mouse_overlay(&mut self, x: i32, y: i32, button: u8) -> bool {
        let (n, top, kind) = if let Some(p) = &self.picker {
            (p.items.len(), p.top, 0)
        } else if let Some(ed) = &self.listed {
            let all = self.list_all(ed.entry);
            (ed.rows(&all).len(), ed.top, 1)
        } else {
            return false;
        };
        let r = self.overlay_rect(n);
        if !r.contains(x, y) {
            if button == 1 {
                self.picker = None;
                self.listed = None;
            }
            return true;
        }
        let vis = self.overlay_visible(n);
        match button {
            4 | 5 => {
                let d: i32 = if button == 4 { -1 } else { 1 };
                let t = (top as i32 + d * 3).clamp(0, n.saturating_sub(vis) as i32) as usize;
                if kind == 0 {
                    self.picker.as_mut().unwrap().top = t;
                } else {
                    self.listed.as_mut().unwrap().top = t;
                }
            }
            1 | 3 => {
                let ry = y - (r.y + 30);
                if ry < 0 {
                    return true;
                }
                let i = top + (ry / self.l.row_h) as usize;
                if i >= n || i >= top + vis {
                    return true;
                }
                if kind == 0 {
                    self.picker.as_mut().unwrap().sel = i;
                    self.pick();
                } else {
                    self.listed.as_mut().unwrap().sel = i;
                    self.list_toggle();
                }
            }
            _ => {}
        }
        true
    }

    fn click_value(&mut self, e: usize, x: i32, button: u8) {
        let st = self.store.entries[e].setting.clone();
        if button == 3 {
            self.reset(e);
            return;
        }
        let vx = self.l.value_x;
        match st.kind {
            Kind::Bool => self.step(e, 1, false),
            Kind::Int | Kind::Float | Kind::Enum => {
                let slider = vx + 150;
                let sw = (self.l.main.right() - 30 - slider).min(160);
                if x < vx + 22 {
                    self.step(e, -1, false);
                } else if st.kind == Kind::Enum {
                    self.activate(e);
                } else if x < slider {
                    self.step(e, 1, false);
                } else if let (Some(lo), Some(hi), true) = (st.min, st.max, x < slider + sw + 4) {
                    // A click on the slider sets the value where it lands.
                    let f = ((x - slider) as f64 / sw.max(1) as f64).clamp(0.0, 1.0);
                    let raw = lo + (hi - lo) * f;
                    let step = st
                        .step
                        .unwrap_or(if st.kind == Kind::Int { 1.0 } else { 0.1 });
                    let v = ((raw / step).round() * step).clamp(lo, hi);
                    let v = if st.kind == Kind::Int {
                        Value::Integer(v.round() as i64)
                    } else {
                        Value::Float((v * 1000.0).round() / 1000.0)
                    };
                    self.commit(e, Some(v), true);
                } else {
                    self.open_editor(e);
                }
            }
            _ => self.activate(e),
        }
    }

    fn poll_worker(&mut self) -> bool {
        let mut any = false;
        while let Ok(r) = self.worker.rx.try_recv() {
            any = true;
            match r {
                Resp::Written(d) => self.written(d),
                Resp::Keys(k, from) => {
                    self.busy = self.busy.saturating_sub(1);
                    trace(&format!("keys|{}|{from}", k.len()));
                    self.keys = Some((k, from));
                    self.rebuild();
                }
                Resp::Extras(x, from) => {
                    self.busy = self.busy.saturating_sub(1);
                    trace(&format!("extras|{}|{from}", x.len()));
                    self.extras = Some((x, from));
                    self.rebuild();
                }
                Resp::Nirux(n) => {
                    self.busy = self.busy.saturating_sub(1);
                    trace(&format!("nirux|{}", n.installed.as_deref().unwrap_or("-")));
                    self.nirux = Some(n);
                    self.rebuild();
                }
                Resp::Versions(v) => {
                    for (b, ver) in v {
                        trace(&format!("version|{b}|{}", ver.as_deref().unwrap_or("-")));
                        self.versions.insert(b, ver);
                    }
                }
                Resp::TerminalDone { after, error } => {
                    if let Some(e) = error {
                        self.status(e, Tone::Err);
                    }
                    match after {
                        After::Extras => {
                            self.extras = None;
                            self.loaded_extras = false;
                        }
                        After::Nirux => {
                            self.nirux = None;
                            self.loaded_nirux = false;
                        }
                    }
                    self.rebuild();
                }
            }
        }
        any
    }
}

/// Byte index of character `i` in `s`.
pub fn byte_at(s: &str, i: usize) -> usize {
    s.char_indices().nth(i).map(|(b, _)| b).unwrap_or(s.len())
}

fn limit_text(st: &schema::Setting, d: i32) -> String {
    let f = |x: f64| schema::fmt_float(x).trim_end_matches(".0").to_string();
    match (d < 0, st.min, st.max) {
        (true, Some(lo), _) => format!("{} is the lowest", f(lo)),
        (false, _, Some(hi)) => format!("{} is the highest", f(hi)),
        _ => "no further".into(),
    }
}

/// A value for the status line.
pub fn short_value(st: &schema::Setting, v: &Value) -> String {
    if st.kind == Kind::Enum {
        if let Some(c) = st.choices.iter().find(|c| store::same(&c.value, v)) {
            return c.label.clone();
        }
    }
    match v {
        Value::Boolean(true) => "on".into(),
        Value::Boolean(false) => "off".into(),
        Value::Array(a) if a.len() > 4 => format!("{} entries", a.len()),
        Value::Array(a) if a.is_empty() => "empty".into(),
        Value::String(s) if s.is_empty() => "empty".into(),
        v => {
            let mut s = schema::plain(v);
            if !st.unit.is_empty() && matches!(st.kind, Kind::Int | Kind::Float) {
                s.push(' ');
                s.push_str(&st.unit);
            }
            s
        }
    }
}

impl App for SettingsApp {
    fn paint(&mut self, f: &mut Frame) {
        crate::draw::paint(self, f);
    }

    fn dirty(&mut self, _out: &mut Vec<Rect>) {}

    fn resize(&mut self, w: i32, h: i32) {
        self.layout(w, h);
        self.scroll_to_sel();
    }

    fn prepare(&mut self, text: &mut Text, theme: &Theme) {
        let key = (
            theme.name.clone(),
            theme.palette.background,
            theme.palette.accent,
            theme.font_px,
        );
        if key != self.theme_key {
            self.c = crate::draw::colors(&theme.palette);
            let px = theme.font_px.round().clamp(11.0, 22.0);
            self.body = text.size(px);
            self.small = text.size((px - 2.0).max(10.0));
            self.theme_key = key;
            let (w, h) = (self.l.w, self.l.h);
            if w > 0 {
                self.layout(w, h);
                self.scroll_to_sel();
            }
        }
    }

    fn key(&mut self, k: &KeyEv) -> Redraw {
        if self.confirm.is_some() {
            let c = self.confirm.take().unwrap();
            if matches!(k.sym, 0xff0d | 0xff8d) || k.ch == Some('y') {
                match c.act {
                    ConfirmAct::Terminal(s, after) => self.terminal(&s, after),
                }
            } else {
                self.status("Cancelled.", Tone::Info);
            }
            return Redraw::Full;
        }
        if self.editor.is_some() {
            self.editor_key(k);
            return Redraw::Full;
        }
        if self.picker.is_some() {
            self.picker_key(k);
            return Redraw::Full;
        }
        if self.listed.is_some() {
            self.listed_key(k);
            return Redraw::Full;
        }
        if self.focus == Focus::Search {
            self.search_key(k);
            return Redraw::Full;
        }
        // Keys that work everywhere else.
        if k.ctrl && matches!(k.ch, Some('q') | Some('w')) {
            self.quit = true;
            return Redraw::None;
        }
        if (k.ctrl && k.ch == Some('z')) || (!k.ctrl && k.ch == Some('u')) {
            self.do_undo();
            return Redraw::Full;
        }
        if (k.ctrl && k.ch == Some('f')) || k.ch == Some('/') {
            self.focus = Focus::Search;
            self.blink = true;
            self.blink_at = Some(Instant::now() + BLINK);
            trace("search|open");
            return Redraw::Full;
        }
        if k.ch == Some('?') {
            self.status(
                "↑↓ move · ←→ or +/- change (Shift: ×10) · Space toggles · Return edits · r resets · u undoes · / searches · 1-9 sections · Tab sidebar · Ctrl+Q quits",
                Tone::Info,
            );
            return Redraw::Full;
        }
        if k.ch == Some('q') && self.focus == Focus::Side {
            self.quit = true;
            return Redraw::None;
        }
        if let Some(c) = k.ch {
            if !k.ctrl && ('1'..='9').contains(&c) {
                self.open_section(c as usize - '1' as usize);
                self.focus = Focus::Main;
                self.trace_sel();
                return Redraw::Full;
            }
        }
        match self.focus {
            Focus::Side => self.side_key(k),
            _ => self.main_key(k),
        }
        Redraw::Full
    }

    fn mouse(&mut self, m: Mouse) -> Redraw {
        match m {
            Mouse::Motion { x, y } => {
                self.mouse = (x, y);
                let h = if self.picker.is_some() || self.listed.is_some() {
                    None
                } else {
                    self.row_at(x, y).filter(|&i| self.rows[i].selectable())
                };
                let hs = self.side_at(x, y);
                if h != self.hover || hs != self.hover_side {
                    if let Some(Row::Set(e)) = h.and_then(|i| self.rows.get(i)) {
                        trace(&format!("hover|{}", cli::id(&self.store, *e)));
                    }
                    self.hover = h;
                    self.hover_side = hs;
                    return Redraw::Full;
                }
                Redraw::None
            }
            Mouse::Leave => {
                self.mouse = (-1, -1);
                if self.hover.is_some() || self.hover_side.is_some() {
                    self.hover = None;
                    self.hover_side = None;
                    return Redraw::Full;
                }
                Redraw::None
            }
            Mouse::Press { x, y, button } => {
                self.confirm = None;
                if self.mouse_overlay(x, y, button) {
                    return Redraw::Full;
                }
                if self.editor.is_some() {
                    // A click elsewhere ends the edit without a change.
                    let inside = self
                        .row_at(x, y)
                        .is_some_and(|i| matches!(self.rows[i], Row::Set(e) if Some(e) == self.editor.as_ref().map(|ed| ed.entry)));
                    if !inside {
                        self.editor = None;
                    } else {
                        return Redraw::None;
                    }
                }
                if self.l.search.contains(x, y) && button == 1 {
                    self.focus = Focus::Search;
                    return Redraw::Full;
                }
                if let Some(s) = self.side_at(x, y) {
                    if button == 1 {
                        self.open_section(s);
                        self.focus = Focus::Side;
                    }
                    return Redraw::Full;
                }
                if let Some(i) = self.row_at(x, y) {
                    match button {
                        4 | 5 => {
                            let d = if button == 4 { -1 } else { 1 };
                            if let (true, Row::Set(e)) = (x >= self.l.value_x, self.rows[i].clone())
                            {
                                let kind = self.store.entries[e].setting.kind;
                                if matches!(kind, Kind::Int | Kind::Float | Kind::Enum) {
                                    self.sel = i;
                                    self.focus = Focus::Main;
                                    self.step(e, -d, false);
                                    return Redraw::Full;
                                }
                            }
                            self.scroll(d * 3);
                        }
                        1 | 3 => {
                            if !self.rows[i].selectable() {
                                return Redraw::None;
                            }
                            self.sel = i;
                            if self.focus == Focus::Search {
                                self.focus = Focus::Main;
                            }
                            self.focus = Focus::Main;
                            match self.rows[i].clone() {
                                Row::Set(e) => {
                                    if x >= self.l.main.right() - 30
                                        && button == 1
                                        && self.modified(e)
                                    {
                                        self.reset(e);
                                    } else if x >= self.l.value_x {
                                        self.click_value(e, x, button);
                                    } else if button == 3 {
                                        self.reset(e);
                                    }
                                }
                                Row::Act(a, _) => self.run_act(&a),
                                Row::Extra(j) if button == 3 => self.extra_action(j, true),
                                Row::Extra(j) if x >= self.l.value_x => self.extra_action(j, false),
                                _ => {}
                            }
                            self.trace_sel();
                        }
                        _ => {}
                    }
                    return Redraw::Full;
                }
                if self.l.rows.contains(x, y) && matches!(button, 4 | 5) {
                    self.scroll(if button == 4 { -3 } else { 3 });
                    return Redraw::Full;
                }
                Redraw::None
            }
        }
    }

    fn poll(&mut self) -> Redraw {
        if self.poll_worker() {
            Redraw::Full
        } else {
            Redraw::None
        }
    }

    fn next_tick(&self) -> Option<Instant> {
        let mut t = self.pending.iter().map(|p| p.at).min();
        if self.has_focus && (self.editor.is_some() || self.focus == Focus::Search) {
            t = match (t, self.blink_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        t
    }

    fn tick(&mut self) -> Redraw {
        let now = Instant::now();
        let mut r = Redraw::None;
        if self.pending.iter().any(|p| p.at <= now) {
            self.flush(false);
            r = Redraw::Full;
        }
        if self.blink_at.is_some_and(|t| t <= now) {
            if self.editor.is_some() || self.focus == Focus::Search {
                self.blink = !self.blink;
                self.blink_at = Some(now + BLINK);
                r = Redraw::Full;
            } else {
                self.blink_at = None;
            }
        }
        r
    }

    fn visible(&mut self, _v: bool) {}

    fn title(&self) -> &str {
        &self.title
    }

    fn pointer(&self) -> Pointer {
        let (x, y) = self.mouse;
        if self.side_at(x, y).is_some()
            || self.l.search.contains(x, y)
            || self.row_at(x, y).is_some_and(|i| self.rows[i].selectable())
        {
            Pointer::Hand
        } else {
            Pointer::Arrow
        }
    }

    fn wants_quit(&self) -> bool {
        self.quit
    }

    fn quit(&mut self) {
        // Write what is still waiting, and wait for it (briefly).
        self.flush(true);
        let until = Instant::now() + Duration::from_secs(5);
        while !self.inflight.is_empty() && Instant::now() < until {
            match self.worker.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Resp::Written(d)) => self.written(d),
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
    }

    fn files_changed(&mut self) -> Redraw {
        // Someone else (NIRUWM saving layouts, an editor, the launcher)
        // changed a file: show it, unless our own change is waiting.
        if !self.pending.is_empty() || !self.inflight.is_empty() {
            return Redraw::None;
        }
        let themes = sources::themes();
        let mut changed = themes != self.themes;
        self.themes = themes;
        self.swatches.clear();
        if self.store.refresh() {
            changed = true;
            trace("files|changed");
            self.refresh_editors();
        }
        if changed {
            self.rebuild();
            Redraw::Full
        } else {
            Redraw::None
        }
    }

    fn request(&mut self, what: &str) -> Redraw {
        trace(&format!("request|{what}"));
        if let Some(s) = section_index(what) {
            self.open_section(s);
            self.focus = Focus::Main;
        }
        Redraw::Full
    }

    fn focused(&mut self, on: bool) -> Redraw {
        self.has_focus = on;
        if on && (self.editor.is_some() || self.focus == Focus::Search) {
            self.blink_at = Some(Instant::now() + BLINK);
        }
        Redraw::Full
    }
}

impl SettingsApp {
    fn scroll(&mut self, d: i32) {
        let vis = self.l.visible.max(1);
        let max_top = self.rows.len().saturating_sub(vis) as i32;
        self.top = (self.top as i32 + d).clamp(0, max_top) as usize;
    }
}

/// A section by name or number: "bar", "lock", "3", "key bindings".
pub fn section_index(what: &str) -> Option<usize> {
    let w = what.trim().to_lowercase();
    if let Ok(n) = w.parse::<usize>() {
        return (1..=SECTIONS.len()).contains(&n).then(|| n - 1);
    }
    let alias = match w.as_str() {
        "theme" | "look" | "appearance" => "appearance",
        "wm" | "niruwm" | "window" | "windows" => "window manager",
        "nirubar" | "bar" => "bar",
        "lock" | "nirulock" => "lock screen",
        "saver" | "idle" | "niruscreensaver" => "screensaver",
        "sound" | "sounds" => "sound",
        "keys" | "bindings" | "shortcuts" => "key bindings",
        "extra" | "extras" => "extras",
        "update" | "updates" | "nirux" => "updates",
        other => other,
    };
    SECTIONS.iter().position(|s| s.to_lowercase() == alias)
}
