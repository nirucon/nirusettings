//! The pages over a section: a list of tables (scratchpad slots, autostart
//! programs, window rules) and the form that edits one entry of it, the
//! program picker that fills a form in, and the key bindings view, which
//! is the form's other user.
//!
//! A form works on a draft: nothing is written until Save (Return on Save,
//! or Ctrl+S). The whole list is then checked by the schema and, when it is
//! written, by the component; a refused save brings the form back.
use super::*;
use nirusettings::desktop::{App as DesktopApp, ClassFrom};
use nirusettings::keys::Binding;
use nirusettings::schema::{Field, Setting, Table};

#[allow(clippy::large_enum_variant)]
pub enum Page {
    /// List `t`; `back` is the selection and scroll to return to.
    Table {
        t: usize,
        back: (usize, usize),
    },
    Form(Form),
}

#[derive(Clone, Debug)]
pub struct Form {
    pub t: usize,
    /// The entry edited: its place in the list (None: a new one).
    pub index: Option<usize>,
    /// Key bindings: the keys of the binding edited (None: a new one).
    pub old_keys: Option<String>,
    /// The fields, in the numbers a person reads.
    pub draft: toml::Table,
    pub orig: toml::Table,
    /// Where the window class came from (a program picked from the list).
    pub class_from: Option<ClassFrom>,
    pub back: (usize, usize),
    pub error: Option<String>,
    /// The person saw that Save replaces a binding, and saves again.
    pub replace_ok: bool,
}

impl Form {
    pub fn dirty(&self) -> bool {
        self.draft != self.orig
    }
}

/// The program picker: every installed program, filtered by typing.
pub struct AppPicker {
    pub filter: String,
    pub sel: usize,
    pub top: usize,
    /// What a choice fills in: the command (and class), or only the class.
    pub class_only: bool,
}

/// Who a binding belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// NIRUWM's built-in binding.
    Builtin,
    /// An own `[[keys]]` entry on a key NIRUWM leaves free.
    Yours,
    /// An own entry in place of a built-in binding.
    Changed,
    /// A program's key (keys.d): shown, changed in the program.
    App(String),
}

/// One binding in effect, as the Key bindings view lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub keys: String,
    pub label: String,
    pub action: Option<String>,
    pub description: String,
    pub group: String,
    pub source: KeySource,
    /// Another binding on the same physical key of this layout.
    pub clash: Option<String>,
    /// That other binding wins: this one does nothing.
    pub lost: bool,
}

const GROUPS: &[&str] = &[
    "Programs",
    "Apps and launchers",
    "Launcher",
    "Session",
    "Layouts",
    "Scratchpads",
    "Windows",
    "Focus and windows",
    "Workspaces",
    "Floating windows",
    "Screenshots and color",
    "Hardware keys",
];

impl SettingsApp {
    // ── Pages ──────────────────────────────────────────────────────────

    pub fn form(&self) -> Option<&Form> {
        match self.pages.last() {
            Some(Page::Form(f)) => Some(f),
            _ => None,
        }
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        match self.pages.last_mut() {
            Some(Page::Form(f)) => Some(f),
            _ => None,
        }
    }

    /// The list a page shows (the form's list for a form).
    pub fn page_table(&self) -> Option<usize> {
        match self.pages.last()? {
            Page::Table { t, .. } => Some(*t),
            Page::Form(f) => Some(f.t),
        }
    }

    pub fn table(&self, t: usize) -> &Table {
        &self.store.tables[t].table
    }

    pub fn keys_table(&self) -> Option<usize> {
        (0..self.store.tables.len()).find(|&t| self.table(t).editor.as_deref() == Some("keys"))
    }

    /// The title of the open page: `[2] WINDOW MANAGER > SCRATCHPADS`.
    pub fn page_title(&self) -> Option<String> {
        let page = self.pages.last()?;
        let t = self.page_table()?;
        let tb = self.table(t);
        let base = format!(
            "[{}] {}",
            self.section + 1,
            SECTIONS[self.section].to_uppercase()
        );
        Some(match page {
            Page::Table { .. } => format!("{base} > {}", tb.label.to_uppercase()),
            Page::Form(f) => {
                let what = if tb.editor.as_deref() == Some("keys") {
                    match &f.old_keys {
                        Some(k) => keys::label(k).to_uppercase(),
                        None => "NEW BINDING".into(),
                    }
                } else if let (Some(id), true) = (
                    tb.id_field.as_ref().and_then(|k| f.draft.get(k)),
                    tb.slots.is_some(),
                ) {
                    format!("{} {}", tb.item.to_uppercase(), schema::plain(id))
                } else if f.index.is_none() {
                    format!("NEW {}", tb.item.to_uppercase())
                } else {
                    tb.item.to_uppercase()
                };
                if tb.section == "Key bindings" {
                    format!("{base} > {what}")
                } else {
                    format!("{base} > {} > {what}", tb.label.to_uppercase())
                }
            }
        })
    }

    pub(crate) fn page_rows(&mut self, rows: &mut Vec<Row>) {
        match self.pages.last() {
            Some(Page::Table { t, .. }) => self.table_rows(*t, rows),
            Some(Page::Form(f)) => {
                let t = f.t;
                let editing = f.index.is_some() || f.old_keys.is_some();
                let tb = self.table(t).clone();
                if tb.fields.iter().any(|f| f.program.is_some()) {
                    rows.push(Row::Act(
                        Act::Pick,
                        "Choose a program from the installed ones (p)".into(),
                    ));
                }
                rows.push(Row::Head(match tb.editor.as_deref() {
                    Some("keys") => "Binding".to_string(),
                    _ => tb.item.clone(),
                }));
                for (k, f) in tb.fields.iter().enumerate() {
                    rows.push(Row::Field(k));
                    if f.setting.key == "command" && tb.editor.as_deref() == Some("scratchpad") {
                        rows.push(Row::Class);
                    }
                }
                rows.push(Row::Head(String::new()));
                rows.push(Row::Act(Act::Save, "Save (Ctrl+S)".into()));
                rows.push(Row::Act(Act::Cancel, "Cancel (Esc)".into()));
                if editing {
                    let what = if tb.editor.as_deref() == Some("keys") {
                        "Remove this binding (a built-in one is released)".to_string()
                    } else {
                        format!("Remove this {}", tb.item)
                    };
                    rows.push(Row::Act(Act::Remove, what));
                }
            }
            None => {}
        }
    }

    fn table_rows(&mut self, t: usize, rows: &mut Vec<Row>) {
        let tb = self.table(t).clone();
        let mut note = tb.help.clone();
        if tb.ordered {
            note.push_str(" Shift+Up/Down moves one.");
        }
        rows.push(Row::Note(note));
        if tb.editor.as_deref() == Some("scratchpad") {
            rows.push(Row::Head("Default scratchpad (Super+Shift+S)".into()));
            let s = self.store.tables[t].schema;
            for e in 0..self.store.entries.len() {
                let en = &self.store.entries[e];
                if en.schema == s && en.setting.key.starts_with("scratchpad.") {
                    rows.push(Row::Set(e));
                }
            }
        }
        rows.push(Row::Head(tb.label.clone()));
        match tb.slots {
            Some([lo, hi]) => {
                for n in lo..=hi {
                    rows.push(Row::Slot(t, n));
                }
            }
            None => {
                let n = self.store.items(t).len();
                if n == 0 {
                    rows.push(Row::Note(format!("No {}s yet.", tb.item)));
                }
                for i in 0..n {
                    rows.push(Row::Item(t, i));
                }
                rows.push(Row::Act(Act::Add(t), format!("Add a {} (a)", tb.item)));
            }
        }
    }

    pub(crate) fn open_table(&mut self, t: usize) {
        let back = (self.sel, self.top);
        self.pages.push(Page::Table { t, back });
        self.sel = 0;
        self.top = 0;
        self.rows.clear();
        self.rebuild();
        self.focus = Focus::Main;
        trace(&format!("page|{}", cli::table_id(&self.store, t)));
        self.status(
            format!(
                "{}: Return edits, a adds, Delete removes, Esc goes back.",
                self.table(t).label
            ),
            Tone::Info,
        );
    }

    /// The form for entry `index` of list `t` (None: a new one, in `slot`).
    pub(crate) fn open_item(&mut self, t: usize, index: Option<usize>, slot: Option<i64>) {
        let tb = self.table(t).clone();
        let mut draft = toml::Table::new();
        match index.and_then(|i| self.store.items(t).get(i).cloned()) {
            Some(item) => {
                for (k, v) in item {
                    let v = tb.field(&k).map(|f| f.shown(&v)).unwrap_or(v);
                    draft.insert(k, v);
                }
            }
            None => {
                for f in &tb.fields {
                    if let (true, Some(d)) = (f.required, &f.setting.default) {
                        draft.insert(f.setting.key.clone(), d.clone());
                    }
                }
                if let (Some(id), Some(n)) = (&tb.id_field, slot) {
                    draft.insert(id.clone(), Value::Integer(n));
                }
            }
        }
        let index = index.filter(|&i| i < self.store.items(t).len());
        self.push_form(Form {
            t,
            index,
            old_keys: None,
            orig: if index.is_some() {
                draft.clone()
            } else {
                toml::Table::new()
            },
            draft,
            class_from: None,
            back: (self.sel, self.top),
            error: None,
            replace_ok: false,
        });
    }

    fn push_form(&mut self, f: Form) {
        let t = f.t;
        let new = f.index.is_none() && f.old_keys.is_none();
        self.pages.push(Page::Form(f));
        self.sel = 0;
        self.top = 0;
        self.rows.clear();
        self.rebuild();
        // Start on the first field a person fills in.
        if let Some(i) = self.rows.iter().position(|r| match r {
            Row::Field(k) => !self.table(t).fields[*k].setting.read_only,
            _ => false,
        }) {
            self.sel = i;
            if new && self.table(t).fields.iter().any(|f| f.program.is_some()) {
                self.sel = 0;
            }
        }
        self.scroll_to_sel();
        trace(&format!("form|{}", cli::table_id(&self.store, t)));
        self.status(
            "Return changes a field · Ctrl+S saves · Esc cancels.",
            Tone::Info,
        );
    }

    pub(crate) fn reopen_form(&mut self, f: Form) {
        let t = f.t;
        if self.page_table() != Some(t) || self.form().is_some() {
            // The list's page went away meanwhile: open it again first.
            if self.page_table() != Some(t) {
                self.pages.clear();
                self.pages.push(Page::Table { t, back: (0, 0) });
            }
        }
        self.push_form(f);
    }

    /// Esc on a page: back one step (a changed form asks first).
    pub(crate) fn close_page(&mut self, ask: bool) {
        if ask && self.form().is_some_and(Form::dirty) {
            self.confirm = Some(Confirm {
                text: "Leave without saving? Return: leave · Esc: keep editing".into(),
                act: ConfirmAct::Discard,
            });
            trace("confirm|discard");
            return;
        }
        let back = match self.pages.pop() {
            Some(Page::Table { back, .. }) => back,
            Some(Page::Form(f)) => f.back,
            None => return,
        };
        self.rows.clear();
        self.rebuild();
        self.sel = back.0.min(self.rows.len().saturating_sub(1));
        self.top = back.1;
        if !self.rows.get(self.sel).is_some_and(Row::selectable) {
            self.move_sel(1);
        }
        self.scroll_to_sel();
        trace("page|back");
    }

    // ── Fields ─────────────────────────────────────────────────────────

    pub fn field_def(&self, k: usize) -> Option<&Field> {
        let f = self.form()?;
        self.table(f.t).fields.get(k)
    }

    pub fn target_setting(&self, t: Target) -> Setting {
        match t {
            Target::Entry(e) => self.store.entries[e].setting.clone(),
            Target::Field(k) => self
                .field_def(k)
                .map(|f| f.setting.clone())
                .unwrap_or_else(|| self.class_setting()),
            Target::Class => self.class_setting(),
        }
    }

    fn class_setting(&self) -> Setting {
        let mut st = self
            .form()
            .and_then(|f| self.table(f.t).field("command"))
            .map(|f| f.setting.clone())
            .expect("a scratchpad form");
        st.key = "class".into();
        st.kind = Kind::String;
        st.label = "Window class".into();
        st.pattern = Some("word".into());
        st.help = "What NIRUWM recognises the slot's window by: its WM_CLASS. A program with --class gets its own; any other one keeps its real class.".into();
        st.min_items = None;
        st
    }

    pub fn target_value(&self, t: Target) -> Option<Value> {
        match t {
            Target::Entry(e) => self.current(e).value,
            Target::Field(k) => {
                let key = &self.field_def(k)?.setting.key;
                self.form()?.draft.get(key).cloned()
            }
            Target::Class => self.form_class().map(|(c, _)| Value::String(c)),
        }
    }

    /// The scratchpad form's window class and where it comes from.
    pub fn form_class(&self) -> Option<(String, Option<ClassFrom>)> {
        let f = self.form()?;
        let cmd: Vec<String> = f
            .draft
            .get("command")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let class = desktop::command_class(&cmd)?;
        Some((class, f.class_from))
    }

    fn set_field(&mut self, k: usize, v: Option<Value>) {
        let Some(def) = self.field_def(k).cloned() else {
            return;
        };
        if let Some(v) = &v {
            if let Err(e) = validate::value(&def.setting, v, &[]) {
                self.status(format!("{}: {e}", def.setting.label), Tone::Err);
                return;
            }
        }
        let key = def.setting.key.clone();
        let Some(f) = self.form_mut() else { return };
        match &v {
            Some(v) => {
                f.draft.insert(key.clone(), v.clone());
            }
            None => {
                f.draft.remove(&key);
            }
        }
        f.error = None;
        f.replace_ok = false;
        if def.program.as_deref() == Some("command") {
            // A typed command: its class is the one it names, if any.
            f.class_from = None;
        }
        let shown = v
            .as_ref()
            .map(|v| short_value(&def.setting, v))
            .unwrap_or_else(|| "unset".into());
        trace(&format!(
            "field|{key}|{}",
            v.as_ref().map(schema::plain).unwrap_or_default()
        ));
        self.status(
            format!("{} = {shown} · Ctrl+S saves", def.setting.label),
            Tone::Info,
        );
    }

    /// Left/Right on a field.
    pub(crate) fn field_step(&mut self, k: usize, d: i32, big: bool) {
        let Some(def) = self.field_def(k).cloned() else {
            return;
        };
        let st = &def.setting;
        if st.read_only {
            self.status(format!("{}: {}", st.label, st.help), Tone::Info);
            return;
        }
        let cur = self.target_value(Target::Field(k));
        match st.kind {
            Kind::Bool if !def.required && st.default.is_none() => {
                // Unset, on, off: unset is NIRUWM's own behaviour.
                let order = [None, Some(true), Some(false)];
                let at = order
                    .iter()
                    .position(|o| *o == cur.as_ref().and_then(Value::as_bool))
                    .unwrap_or(0) as i32;
                let next = order[(at + d).rem_euclid(3) as usize];
                self.set_field(k, next.map(Value::Boolean));
            }
            Kind::Bool => {
                let b = cur.as_ref().and_then(Value::as_bool).unwrap_or(false);
                self.set_field(k, Some(Value::Boolean(!b)));
            }
            Kind::Int | Kind::Float => {
                let n = cur
                    .as_ref()
                    .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
                let base = n.or(st
                    .default
                    .as_ref()
                    .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))));
                let mut step = st
                    .step
                    .unwrap_or(if st.kind == Kind::Int { 1.0 } else { 0.1 });
                if big {
                    step *= 10.0;
                }
                let lo = st.min.unwrap_or(f64::MIN);
                let hi = st.max.unwrap_or(f64::MAX);
                let v = match base {
                    // From unset, the first step lands on the lowest (or
                    // highest) value.
                    None if d > 0 => st.min.unwrap_or(0.0),
                    None => st.max.unwrap_or(0.0),
                    Some(b) => (((b + step * d as f64) / step).round() * step).clamp(lo, hi),
                };
                // Below the lowest value of an optional number is unset.
                if n.is_some() && !def.required && d < 0 && n == st.min {
                    self.set_field(k, None);
                    return;
                }
                let v = if st.kind == Kind::Int {
                    Value::Integer(v.round() as i64)
                } else {
                    Value::Float((v * 1000.0).round() / 1000.0)
                };
                self.set_field(k, Some(v));
            }
            Kind::Enum => {
                let n = st.choices.len() as i32;
                if n == 0 {
                    return;
                }
                let at = cur
                    .as_ref()
                    .and_then(|v| st.choices.iter().position(|c| store::same(&c.value, v)))
                    .map(|i| i as i32)
                    .unwrap_or(-1);
                let next = (at + d).rem_euclid(n) as usize;
                self.set_field(k, Some(st.choices[next].value.clone()));
            }
            _ => self.field_activate(k),
        }
    }

    /// Return on a field: the editor that fits it.
    pub(crate) fn field_activate(&mut self, k: usize) {
        let Some(def) = self.field_def(k).cloned() else {
            return;
        };
        let st = &def.setting;
        if st.read_only {
            self.status(format!("{}: {}", st.label, st.help), Tone::Info);
            return;
        }
        match st.kind {
            Kind::Bool | Kind::Int | Kind::Float if st.kind == Kind::Bool => {
                self.field_step(k, 1, false)
            }
            Kind::Enum => {
                let cur = self.target_value(Target::Field(k));
                let items: Vec<(Value, String, String)> = st
                    .choices
                    .iter()
                    .map(|c| (c.value.clone(), c.label.clone(), c.help.clone()))
                    .collect();
                let sel = cur
                    .and_then(|v| items.iter().position(|(c, _, _)| store::same(c, &v)))
                    .unwrap_or(0);
                self.picker = Some(Picker {
                    target: Target::Field(k),
                    items,
                    sel,
                    top: 0,
                });
            }
            _ if st.pattern.as_deref() == Some("action") => self.action_picker(k),
            _ if def.program.as_deref() == Some("command")
                && self
                    .target_value(Target::Field(k))
                    .and_then(|v| v.as_array().map(|a| a.is_empty()))
                    .unwrap_or(true) =>
            {
                // An empty program field starts with the list of programs.
                self.open_app_picker(false);
            }
            _ => self.field_edit_text(Target::Field(k)),
        }
    }

    /// Delete on an optional field: unset.
    pub(crate) fn form_unset(&mut self, k: usize) {
        self.set_field(k, None);
    }

    /// The text editor on a field (or the class).
    pub(crate) fn field_edit_text(&mut self, target: Target) {
        let st = self.target_setting(target);
        if st.read_only {
            return;
        }
        let text = validate::as_input(&st, self.target_value(target).as_ref());
        let cur = text.chars().count();
        self.editor = Some(Editor {
            target,
            text,
            cur,
            error: None,
        });
        self.blink = true;
        self.blink_at = Some(Instant::now() + BLINK);
        trace(&format!("editor|{}", st.key));
    }

    /// Return in a field's text editor.
    pub(crate) fn form_text(&mut self, target: Target, text: &str) {
        let st = self.target_setting(target);
        let parsed = if target == Target::Class {
            let t = text.trim();
            match validate::pattern("word", t) {
                Ok(()) => Ok(Some(Value::String(t.to_string()))),
                Err(e) => Err(e),
            }
        } else {
            validate::parse_input(&st, text, &[])
        };
        match parsed {
            Err(err) => {
                trace(&format!("invalid|{}|{err}", st.key));
                if let Some(ed) = &mut self.editor {
                    ed.error = Some(err);
                }
            }
            Ok(v) => {
                self.editor = None;
                match target {
                    Target::Field(k) => self.set_field(k, v),
                    Target::Class => self.set_class(v.and_then(|v| v.as_str().map(str::to_string))),
                    Target::Entry(_) => {}
                }
            }
        }
    }

    /// A new class for the scratchpad form: written into its command.
    fn set_class(&mut self, class: Option<String>) {
        let Some(class) = class else { return };
        let Some(k) = self.form().and_then(|f| {
            self.table(f.t)
                .fields
                .iter()
                .position(|x| x.setting.key == "command")
        }) else {
            return;
        };
        let cmd: Vec<String> = self
            .target_value(Target::Field(k))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        if cmd.is_empty() {
            self.status(
                "Choose the program first: the class goes into its command.",
                Tone::Warn,
            );
            return;
        }
        let new = desktop::with_class(&cmd, &class);
        self.set_field(
            k,
            Some(Value::Array(new.into_iter().map(Value::String).collect())),
        );
        if let Some(f) = self.form_mut() {
            f.class_from = Some(ClassFrom::Command);
        }
    }

    /// A choice in the picker for a field.
    pub(crate) fn form_pick(&mut self, target: Target, v: Value) {
        let Target::Field(k) = target else { return };
        let is_action = self
            .field_def(k)
            .is_some_and(|f| f.setting.pattern.as_deref() == Some("action"));
        if is_action {
            let verb = v.as_str().unwrap_or("").to_string();
            let arg = keys::ACTIONS
                .iter()
                .find(|(n, _, _)| *n == verb)
                .map(|(_, a, _)| *a)
                .unwrap_or("");
            if arg.is_empty() || arg == "[N]" {
                self.set_field(k, Some(Value::String(verb)));
            } else {
                // The argument is typed: the editor opens after the name.
                let text = format!("{verb} ");
                let cur = text.chars().count();
                self.editor = Some(Editor {
                    target,
                    text,
                    cur,
                    error: None,
                });
                self.blink = true;
                self.blink_at = Some(Instant::now() + BLINK);
                self.status(
                    format!(
                        "{verb} {arg}: type the {}, then Return.",
                        arg.to_lowercase()
                    ),
                    Tone::Info,
                );
            }
            return;
        }
        self.set_field(k, Some(v));
    }

    fn action_picker(&mut self, k: usize) {
        let cur = self
            .target_value(Target::Field(k))
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let verb = cur.split_whitespace().next().unwrap_or("").to_string();
        let items: Vec<(Value, String, String)> = keys::ACTIONS
            .iter()
            .map(|(n, a, d)| {
                let label = if a.is_empty() {
                    n.to_string()
                } else {
                    format!("{n} {a}")
                };
                (Value::String(n.to_string()), label, d.to_string())
            })
            .collect();
        let sel = items
            .iter()
            .position(|(v, _, _)| v.as_str() == Some(verb.as_str()))
            .unwrap_or(0);
        self.picker = Some(Picker {
            target: Target::Field(k),
            items,
            sel,
            top: 0,
        });
        trace("picker|action");
    }

    /// True when row `i` is the one the text editor is on.
    pub(crate) fn editor_row(&self, i: usize) -> bool {
        let Some(ed) = &self.editor else { return false };
        match (&self.rows[i], ed.target) {
            (Row::Set(e), Target::Entry(x)) => *e == x,
            (Row::Field(k), Target::Field(x)) => *k == x,
            (Row::Class, Target::Class) => true,
            _ => false,
        }
    }

    // ── Saving ─────────────────────────────────────────────────────────

    /// Write list `t` as `items` (stored numbers), after the schema's
    /// checks; shown at once, written after a moment like every change.
    pub(crate) fn commit_table(&mut self, t: usize, items: Vec<toml::Table>, record: bool) -> bool {
        let tb = self.table(t).clone();
        if let Err(e) = validate::table(&tb, &items) {
            self.status(
                format!("{}: {e}. Nothing was changed.", tb.label),
                Tone::Err,
            );
            return false;
        }
        let before = self.store.items(t);
        if before == items {
            return false;
        }
        let fi = self.store.tables[t].file;
        let change = self.store.table_change(t, &items);
        if let Err(err) = self.store.apply_local(fi, std::slice::from_ref(&change)) {
            self.status(err, Tone::Err);
            return false;
        }
        if record {
            self.undo.push(Undo {
                was: Was::Table(t, before),
                job: 0,
            });
        }
        let apply = cli::table_mode(&self.store, t).run.clone();
        self.queue(fi, vec![change], apply, None, Some(t));
        self.status(format!("{} · saving...", tb.label), Tone::Info);
        true
    }

    /// The form's draft in the file's numbers, in the schema's order, with
    /// the keys the schema does not know kept at the end.
    fn stored_item(&self, f: &Form) -> toml::Table {
        let tb = self.table(f.t);
        let mut out = toml::Table::new();
        for fd in &tb.fields {
            if let Some(v) = f.draft.get(&fd.setting.key) {
                // An empty optional list or text is the same as unset.
                let empty = match v {
                    Value::Array(a) => a.is_empty() && !fd.required,
                    Value::String(s) => s.is_empty() && !fd.required,
                    _ => false,
                };
                if !empty {
                    out.insert(fd.setting.key.clone(), fd.stored(v));
                }
            }
        }
        for (k, v) in &f.draft {
            if tb.field(k).is_none() {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }

    pub(crate) fn form_save(&mut self) {
        let Some(f) = self.form().cloned() else {
            return;
        };
        let tb = self.table(f.t).clone();
        if tb.editor.as_deref() == Some("keys") {
            self.key_save(f);
            return;
        }
        if let Err(e) = validate::item(&tb, &f.draft) {
            self.form_error(e);
            return;
        }
        let item = self.stored_item(&f);
        let mut items = self.store.items(f.t);
        match (f.index, &tb.id_field) {
            (Some(i), _) if i < items.len() => items[i] = item.clone(),
            (_, Some(id)) => {
                // A slot: in its place by number, replacing one there.
                let n = item.get(id).and_then(Value::as_integer).unwrap_or(0);
                items.retain(|x| x.get(id).and_then(Value::as_integer) != Some(n));
                let at = items
                    .iter()
                    .position(|x| x.get(id).and_then(Value::as_integer).unwrap_or(0) > n)
                    .unwrap_or(items.len());
                items.insert(at, item.clone());
            }
            _ => items.push(item.clone()),
        }
        if let Err(e) = validate::table(&tb, &items) {
            self.form_error(e);
            return;
        }
        if self.store.items(f.t) == items {
            self.close_page(false);
            self.status("Not changed.", Tone::Info);
            return;
        }
        self.refused = Some((0, f.clone()));
        self.close_page(false);
        if self.commit_table(f.t, items, true) {
            trace(&format!("formsaved|{}", cli::table_id(&self.store, f.t)));
            if f.class_from == Some(ClassFrom::Guess) {
                self.status(
                    format!(
                        "{} · saving... The window class is a guess: if the window is not found, run xprop WM_CLASS, click it, and type the class here.",
                        tb.label
                    ),
                    Tone::Warn,
                );
            }
        } else {
            self.refused = None;
        }
    }

    fn form_error(&mut self, e: String) {
        trace(&format!("formerror|{e}"));
        if let Some(f) = self.form_mut() {
            f.error = Some(e.clone());
        }
        self.status(format!("Not saved: {e}"), Tone::Err);
    }

    /// Remove the entry the form edits.
    pub(crate) fn form_remove(&mut self) {
        let Some(f) = self.form().cloned() else {
            return;
        };
        let tb = self.table(f.t).clone();
        if tb.editor.as_deref() == Some("keys") {
            if let Some(k) = f.old_keys.clone() {
                self.close_page(false);
                self.key_remove_keys(&k);
            }
            return;
        }
        let Some(i) = f.index else { return };
        self.close_page(false);
        self.item_remove(f.t, i);
    }

    /// Delete on an entry of a list.
    pub(crate) fn item_remove(&mut self, t: usize, i: usize) {
        let mut items = self.store.items(t);
        if i >= items.len() {
            return;
        }
        let gone = items.remove(i);
        let tb = self.table(t).clone();
        if self.commit_table(t, items, true) {
            let (what, _) = self.item_summary(t, &gone);
            trace(&format!("removed|{}|{i}", cli::table_id(&self.store, t)));
            self.status(
                format!("Removed {} {what} · saving... · u undoes", tb.item),
                Tone::Info,
            );
            self.rebuild();
        }
    }

    /// Shift+Up/Down on an entry of an ordered list.
    pub(crate) fn item_move(&mut self, t: usize, i: usize, d: i32) {
        let mut items = self.store.items(t);
        let j = i as i32 + d;
        if !self.table(t).ordered || j < 0 || j as usize >= items.len() {
            return;
        }
        items.swap(i, j as usize);
        if self.commit_table(t, items, true) {
            self.rebuild();
            if let Some(r) = self
                .rows
                .iter()
                .position(|r| *r == Row::Item(t, j as usize))
            {
                self.sel = r;
                self.scroll_to_sel();
            }
        }
    }

    /// The slot's entry in its list, if it has one.
    pub fn slot_index(&self, t: usize, n: i64) -> Option<usize> {
        let id = self.table(t).id_field.as_deref()?;
        self.store
            .items(t)
            .iter()
            .position(|x| x.get(id).and_then(Value::as_integer) == Some(n))
    }

    /// One entry in words: (what it is, the rest).
    pub fn item_summary(&self, t: usize, item: &toml::Table) -> (String, String) {
        let tb = self.table(t);
        let argv = |k: &str| -> Vec<String> {
            item.get(k)
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let size = || {
            let w = item.get("width").map(schema::plain);
            let h = item.get("height").map(schema::plain);
            match (w, h) {
                (None, None) => String::new(),
                (w, h) => format!(
                    " · {} x {}",
                    w.unwrap_or_else(|| "default".into()),
                    h.unwrap_or_else(|| "default".into())
                ),
            }
        };
        match tb.editor.as_deref() {
            Some("scratchpad") => {
                let cmd = argv("command");
                if cmd.is_empty() {
                    (
                        "empty".into(),
                        format!("holds the windows you send to it{}", size()),
                    )
                } else {
                    let class = desktop::command_class(&cmd).unwrap_or_else(|| "no class!".into());
                    (
                        desktop::program_of(&cmd).unwrap_or_default(),
                        format!("{} · class {class}{}", desktop::shown_command(&cmd), size()),
                    )
                }
            }
            Some("autostart") => {
                let cmd = argv("command");
                let ws = item.get("workspace").map(schema::plain).unwrap_or_default();
                let class = item
                    .get("class")
                    .and_then(|c| c.as_str())
                    .map(|c| format!(" · waits for {c}"))
                    .unwrap_or_default();
                (
                    format!("WS {ws}  {}", desktop::program_of(&cmd).unwrap_or_default()),
                    format!("{}{class}", desktop::shown_command(&cmd)),
                )
            }
            Some("rules") => {
                let class = item.get("class").map(schema::plain).unwrap_or_default();
                let mut what: Vec<String> = Vec::new();
                if let Some(ws) = item.get("workspace").and_then(Value::as_integer) {
                    let f = tb.field("workspace").map(|f| f.offset).unwrap_or(0);
                    what.push(format!("workspace {}", ws + f));
                }
                for (k, yes, no) in [
                    ("floating", "floating", "tiled"),
                    ("center", "centred", "not centred"),
                    ("sticky", "on every workspace", "on one workspace"),
                ] {
                    if let Some(b) = item.get(k).and_then(Value::as_bool) {
                        what.push(if b { yes } else { no }.to_string());
                    }
                }
                if let Some(o) = item.get("opacity").and_then(Value::as_integer) {
                    what.push(format!("{o} % opaque"));
                }
                if what.is_empty() {
                    what.push("nothing set yet".into());
                }
                (class, what.join(" · "))
            }
            _ => {
                let mut it = tb.fields.iter().filter_map(|f| {
                    item.get(&f.setting.key).map(|v| {
                        (
                            f.setting.label.clone(),
                            short_value(&f.setting, &f.shown(v)),
                        )
                    })
                });
                let first = it.next().map(|(_, v)| v).unwrap_or_default();
                let rest: Vec<String> = it.map(|(l, v)| format!("{l} {v}")).collect();
                (first, rest.join(" · "))
            }
        }
    }

    /// The summary of a list for its row in the section.
    pub fn table_line(&self, t: usize) -> String {
        let items = self.store.items(t);
        let tb = self.table(t);
        if items.is_empty() {
            return match tb.slots {
                Some(_) => "no programs: every slot holds the windows you send".into(),
                None => format!("no {}s", tb.item),
            };
        }
        let names: Vec<String> = items
            .iter()
            .map(|i| {
                let (a, _) = self.item_summary(t, i);
                match (tb.slots, tb.id_of(i)) {
                    (Some(_), Some(id)) => format!("{id} {a}"),
                    _ => a,
                }
            })
            .collect();
        format!(
            "{}: {}",
            cli::table_summary(&self.store, t),
            names.join(" · ")
        )
    }

    // ── The program picker ─────────────────────────────────────────────

    pub(crate) fn open_app_picker(&mut self, class_only: bool) {
        if !self.loaded_apps {
            self.loaded_apps = true;
            self.busy += 1;
            let _ = self.worker.tx.send(Req::Apps);
        }
        self.app_picker = Some(AppPicker {
            filter: String::new(),
            sel: 0,
            top: 0,
            class_only,
        });
        trace("apps|open");
    }

    /// The programs the filter lets through (indexes into `apps`).
    pub fn picked_apps(&self) -> Vec<usize> {
        let Some(apps) = &self.apps else {
            return Vec::new();
        };
        let f = self
            .app_picker
            .as_ref()
            .map(|p| p.filter.to_lowercase())
            .unwrap_or_default();
        (0..apps.len())
            .filter(|&i| {
                let a = &apps[i];
                f.is_empty()
                    || format!("{} {} {}", a.name, a.id, a.exec.join(" "))
                        .to_lowercase()
                        .contains(&f)
            })
            .collect()
    }

    pub(crate) fn app_picker_key(&mut self, k: &KeyEv) {
        let n = self.picked_apps().len() + 1;
        let Some(p) = &mut self.app_picker else {
            return;
        };
        match k.sym {
            0xff1b => {
                self.app_picker = None;
            }
            0xff52 => p.sel = p.sel.saturating_sub(1),
            0xff54 => p.sel = (p.sel + 1).min(n - 1),
            0xff55 => p.sel = p.sel.saturating_sub(8),
            0xff56 => p.sel = (p.sel + 8).min(n - 1),
            0xff50 if p.filter.is_empty() => p.sel = 0,
            0xff57 if p.filter.is_empty() => p.sel = n - 1,
            0xff0d | 0xff8d => {
                let sel = p.sel;
                self.app_choose(sel);
            }
            0xff08 => {
                p.filter.pop();
                p.sel = if p.filter.is_empty() { 0 } else { 1 };
                p.top = 0;
            }
            _ if k.ctrl && k.ch == Some('u') => {
                p.filter.clear();
                p.sel = 0;
            }
            _ => {
                if let Some(c) = k.ch.filter(|c| !c.is_control() && !k.ctrl) {
                    if p.filter.len() < 40 {
                        p.filter.push(c);
                        // The first match, not the "type a command" row.
                        p.sel = 1;
                        p.top = 0;
                        trace(&format!("apps|filter|{}", p.filter));
                    }
                }
            }
        }
        if let Some(p) = &mut self.app_picker {
            p.sel = p.sel.min(n - 1);
        }
    }

    /// Row `sel` of the program picker: 0 types a command, the rest are
    /// programs.
    pub(crate) fn app_choose(&mut self, sel: usize) {
        let list = self.picked_apps();
        let Some(p) = self.app_picker.take() else {
            return;
        };
        let Some(f) = self.form().cloned() else {
            return;
        };
        let tb = self.table(f.t).clone();
        let field = |what: &str| {
            tb.fields
                .iter()
                .position(|x| x.program.as_deref() == Some(what))
        };
        if sel == 0 || list.is_empty() {
            // Type it instead.
            let target = if p.class_only {
                field("class").map(Target::Field)
            } else {
                field("command").map(Target::Field)
            };
            if let Some(t) = target {
                self.field_edit_text(t);
            }
            return;
        }
        let Some(app) = self
            .apps
            .as_ref()
            .and_then(|a| a.get(list[sel - 1]).cloned())
        else {
            return;
        };
        trace(&format!("apps|chose|{}", app.id));
        let terminal = self
            .store
            .entries
            .iter()
            .position(|en| {
                en.setting.key == "terminal" && self.store.schemas[en.schema].component == "niruwm"
            })
            .map(|e| as_string_list(self.current(e).value.as_ref()))
            .unwrap_or_default();
        let list_value = |v: Vec<String>| Value::Array(v.into_iter().map(Value::String).collect());
        let mut note = String::new();
        let mut from = None;
        match tb.editor.as_deref() {
            Some("scratchpad") if !p.class_only => {
                let (cmd, class) = desktop::scratchpad_command(&app, &terminal);
                if let Some(k) = field("command") {
                    self.set_field(k, Some(list_value(cmd)));
                }
                if let Some((c, fr)) = class {
                    note = format!("window class {c}, {}", fr.words());
                    from = Some(fr);
                }
            }
            _ => {
                if !p.class_only {
                    if let Some(k) = field("command") {
                        let mut cmd = app.exec.clone();
                        if app.terminal && !terminal.is_empty() {
                            let mut t = terminal.clone();
                            t.push("-e".into());
                            t.extend(cmd);
                            cmd = t;
                        }
                        self.set_field(k, Some(list_value(cmd)));
                    }
                }
                if let (Some(k), Some((c, fr))) =
                    (field("class"), desktop::class_for(&app.exec, Some(&app)))
                {
                    let c = if fr == ClassFrom::Command {
                        c
                    } else {
                        c.to_string()
                    };
                    self.set_field(k, Some(Value::String(c.clone())));
                    note = format!("window class {c}, {}", fr.words());
                    from = Some(fr);
                }
            }
        }
        if let Some(f) = self.form_mut() {
            f.class_from = from;
        }
        let tone = if from == Some(ClassFrom::Guess) {
            Tone::Warn
        } else {
            Tone::Info
        };
        self.status(format!("{}: {note} · Ctrl+S saves", app.name), tone);
        // On to Save.
        if let Some(i) = self
            .rows
            .iter()
            .position(|r| *r == Row::Act(Act::Save, "Save (Ctrl+S)".into()))
        {
            self.sel = i;
            self.scroll_to_sel();
        }
    }

    /// A program's icon, 20 px, read once.
    pub fn icon(&mut self, app: &DesktopApp) -> Option<Vec<u32>> {
        if let Some(i) = self.icons.get(&app.icon) {
            return i.clone();
        }
        let px = desktop::icon_pixels(&app.icon, 20);
        self.icons.insert(app.icon.clone(), px.clone());
        px
    }

    // ── Key bindings ───────────────────────────────────────────────────

    /// The `[[keys]]` entries of the file.
    fn user_keys(&self) -> Vec<toml::Table> {
        self.keys_table()
            .map(|t| self.store.items(t))
            .unwrap_or_default()
    }

    /// NIRUWM's built-in bindings: from niruwm, else the ones niructl says
    /// are in effect and come from NIRUWM.
    fn builtin_keys(&self) -> Vec<Binding> {
        if let Some(d) = &self.key_defaults {
            return d.clone();
        }
        let Some((sheet, _)) = &self.keys else {
            return Vec::new();
        };
        sheet
            .iter()
            .filter(|k| k.source != "config" && k.source != "app")
            .filter_map(|k| {
                Some(Binding {
                    keys: k.keys.clone(),
                    action: k.action.clone()?,
                    description: None,
                })
            })
            .collect()
    }

    /// The bindings in effect, computed as NIRUWM does from its built-in
    /// table and the file, with the words niructl has for them.
    pub(crate) fn rebuild_key_view(&mut self) {
        let defaults = self.builtin_keys();
        let user: Vec<Binding> = self
            .user_keys()
            .iter()
            .filter_map(Binding::from_table)
            .collect();
        let eff = keys::effective(&defaults, &user);
        let sheet: Vec<sources::Key> = self
            .keys
            .as_ref()
            .map(|(k, _)| k.clone())
            .unwrap_or_default();
        let clashes = keys::shadowed(&eff, &self.layout_keys);
        let mut out: Vec<KeyRow> = Vec::new();
        for (i, b) in eff.iter().enumerate() {
            if b.unbinds() {
                continue;
            }
            let known = sheet.iter().find(|k| {
                keys::same_keys(&k.keys, &b.keys) && k.action.as_deref() == Some(b.action.as_str())
            });
            let builtin = defaults.iter().any(|d| keys::same_keys(&d.keys, &b.keys));
            let own = user.iter().any(|u| keys::same_keys(&u.keys, &b.keys));
            let same_as_builtin = defaults
                .iter()
                .any(|d| keys::same_keys(&d.keys, &b.keys) && d.action == b.action);
            let source = match (own, builtin) {
                (true, true) if same_as_builtin => KeySource::Builtin,
                (true, true) => KeySource::Changed,
                (true, false) => KeySource::Yours,
                _ => KeySource::Builtin,
            };
            let description = b
                .description
                .clone()
                .filter(|d| !d.trim().is_empty())
                .or_else(|| {
                    known
                        .map(|k| k.description.clone())
                        .filter(|d| !d.is_empty())
                })
                .unwrap_or_else(|| keys::describe(&b.action));
            let group = known
                .map(|k| k.group.clone())
                .filter(|g| !g.is_empty())
                .unwrap_or_else(|| sources::group_of(&b.action).to_string());
            let (clash, lost) = match clashes.iter().find(|(d, k)| *d == i || *k == i) {
                Some((d, k)) if *d == i => (
                    Some(format!(
                        "{} is the same key as {} on this keyboard layout; {} is used",
                        b.keys, eff[*k].keys, eff[*k].keys
                    )),
                    true,
                ),
                Some((d, _)) => (
                    Some(format!(
                        "{} is the same key on this keyboard layout; this one is used",
                        eff[*d].keys
                    )),
                    false,
                ),
                None => (None, false),
            };
            out.push(KeyRow {
                keys: b.keys.clone(),
                label: keys::label(&b.keys),
                action: Some(b.action.clone()),
                description,
                group,
                source,
                clash,
                lost,
            });
        }
        for k in sheet.iter().filter(|k| k.source == "app") {
            out.push(KeyRow {
                keys: k.keys.clone(),
                label: k.label.clone(),
                action: None,
                description: k.description.clone(),
                group: if k.group.is_empty() {
                    "Apps".into()
                } else {
                    k.group.clone()
                },
                source: KeySource::App(k.source.clone()),
                clash: None,
                lost: false,
            });
        }
        let rank = |g: &str| GROUPS.iter().position(|x| *x == g).unwrap_or(GROUPS.len());
        out.sort_by_key(|k| (rank(&k.group), k.group.clone()));
        self.key_view = out;
    }

    pub(crate) fn key_rows(&mut self, rows: &mut Vec<Row>) {
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
        let Some((_, from)) = &self.keys else {
            rows.push(Row::Note("Asking niructl for the key bindings...".into()));
            return;
        };
        let from = from.clone();
        self.rebuild_key_view();
        let lost = self.key_view.iter().filter(|k| k.lost).count();
        let mut note = format!(
            "{} bindings, from {from}. Return changes one, a adds, Delete removes, r puts NIRUWM's back.",
            self.key_view.len()
        );
        if lost > 0 {
            note =
                format!("{lost} binding(s) do nothing on this keyboard layout (marked !). {note}");
        }
        rows.push(Row::Note(note));
        rows.push(Row::Act(Act::AddKey, "Add a key binding (a)".into()));
        let mut group = String::new();
        for i in 0..self.key_view.len() {
            if self.key_view[i].group != group {
                group = self.key_view[i].group.clone();
                rows.push(Row::Head(group.clone()));
            }
            rows.push(Row::Key(i));
        }
        let defaults = self.builtin_keys();
        let released: Vec<usize> = self
            .user_keys()
            .iter()
            .enumerate()
            .filter_map(|(j, t)| {
                let b = Binding::from_table(t)?;
                (b.unbinds() && defaults.iter().any(|d| keys::same_keys(&d.keys, &b.keys)))
                    .then_some(j)
            })
            .collect();
        if !released.is_empty() {
            rows.push(Row::Head("Released built-in keys".into()));
            for j in released {
                rows.push(Row::Released(j));
            }
        }
    }

    /// What a released key did before: NIRUWM's built-in binding.
    pub fn released_info(&self, j: usize) -> Option<(String, String)> {
        let t = self.user_keys().get(j).cloned()?;
        let b = Binding::from_table(&t)?;
        let d = self
            .builtin_keys()
            .into_iter()
            .find(|d| keys::same_keys(&d.keys, &b.keys))?;
        Some((keys::label(&b.keys), keys::describe(&d.action)))
    }

    /// The form for binding `i` of the view (None: a new binding).
    pub(crate) fn open_key(&mut self, i: Option<usize>) {
        let Some(t) = self.keys_table() else {
            self.status("The schema has no [[keys]] list.", Tone::Err);
            return;
        };
        let row = i.and_then(|i| self.key_view.get(i).cloned());
        if let Some(KeySource::App(app)) = row.as_ref().map(|r| r.source.clone()) {
            self.status(
                format!(
                    "{}: a program's own key ({app}); change it in that program.",
                    row.map(|r| r.label).unwrap_or_default()
                ),
                Tone::Info,
            );
            return;
        }
        let mut draft = toml::Table::new();
        if let Some(r) = &row {
            draft.insert("keys".into(), Value::String(r.keys.clone()));
            draft.insert(
                "action".into(),
                Value::String(r.action.clone().unwrap_or_default()),
            );
            // An own description stays; NIRUWM's own words are not copied.
            let own = self
                .user_keys()
                .iter()
                .filter_map(Binding::from_table)
                .find(|b| keys::same_keys(&b.keys, &r.keys))
                .and_then(|b| b.description);
            if let Some(d) = own {
                draft.insert("description".into(), Value::String(d));
            }
        }
        if self.pages.is_empty() {
            // The bindings view is the list page of the keys form.
            self.pages.push(Page::Table {
                t,
                back: (self.sel, self.top),
            });
        }
        self.push_form(Form {
            t,
            index: None,
            old_keys: row.map(|r| r.keys),
            orig: draft.clone(),
            draft,
            class_from: None,
            back: (self.sel, self.top),
            error: None,
            replace_ok: false,
        });
        if self.form().is_some_and(|f| f.old_keys.is_none()) {
            // A new binding starts in the keys field.
            if let Some(r) = self.rows.iter().position(|r| *r == Row::Field(0)) {
                self.sel = r;
            }
        }
    }

    /// What Save would replace or shadow for the form's keys.
    pub fn key_conflicts(&self) -> (Option<String>, Option<String>) {
        let Some(f) = self.form() else {
            return (None, None);
        };
        let Some(k) = f.draft.get("keys").and_then(|v| v.as_str()) else {
            return (None, None);
        };
        if keys::parse_keys(k).is_err() {
            return (None, None);
        }
        let old = f.old_keys.clone();
        let taken = self.key_view.iter().find(|r| {
            keys::same_keys(&r.keys, k) && !old.as_deref().is_some_and(|o| keys::same_keys(o, k))
        });
        let replace = taken.map(|r| {
            format!(
                "{} is {} now ({}); saving replaces it",
                keys::label(k),
                r.description,
                match &r.source {
                    KeySource::App(a) => format!("{a}'s key"),
                    KeySource::Builtin => "NIRUWM's".into(),
                    _ => "yours".into(),
                }
            )
        });
        let physical = self.layout_keys.key_of(k).and_then(|(code, mods, level)| {
            self.key_view.iter().find_map(|r| {
                if keys::same_keys(&r.keys, k) || old.as_deref().is_some_and(|o| keys::same_keys(o, &r.keys)) {
                    return None;
                }
                let (c2, m2, l2) = self.layout_keys.key_of(&r.keys)?;
                (c2 == code && m2 == mods).then(|| {
                    if level < l2 {
                        format!("{k} is the same key as {} on this keyboard layout; {k} is used", r.keys)
                    } else {
                        format!(
                            "{k} is the same key as {} on this keyboard layout; {} is used, so this one does nothing",
                            r.keys, r.keys
                        )
                    }
                })
            })
        });
        (replace, physical)
    }

    fn key_save(&mut self, f: Form) {
        let Some(t) = self.keys_table() else { return };
        let tb = self.table(t).clone();
        if let Err(e) = validate::item(&tb, &f.draft) {
            self.form_error(e);
            return;
        }
        let new = Binding {
            keys: keys::normal(f.draft.get("keys").and_then(|v| v.as_str()).unwrap_or("")),
            action: f
                .draft
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            description: f
                .draft
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|d| !d.trim().is_empty()),
        };
        let (replace, physical) = self.key_conflicts();
        if let (Some(r), false) = (&replace, f.replace_ok) {
            trace(&format!("keyconflict|{r}"));
            if let Some(ff) = self.form_mut() {
                ff.replace_ok = true;
                ff.error = Some(format!("{r}. Ctrl+S again saves."));
            }
            self.status(
                format!("{r}. Ctrl+S again saves, Esc keeps it."),
                Tone::Warn,
            );
            return;
        }
        let user = self.user_keys();
        let items = keys::set_binding(&user, &self.builtin_keys(), f.old_keys.as_deref(), &new);
        if let Err(e) = validate::table(&tb, &items) {
            self.form_error(e);
            return;
        }
        self.refused = Some((0, f.clone()));
        self.close_page(false);
        if self.commit_table(t, items, true) {
            trace(&format!("keysaved|{}|{}", new.keys, new.action));
            self.rebuild();
            if let Some(p) = physical {
                trace(&format!("keyshadow|{p}"));
                self.status(
                    format!("{} · saving... · {p}", keys::label(&new.keys)),
                    Tone::Warn,
                );
            } else {
                self.status(
                    format!(
                        "{}: {} · saving...",
                        keys::label(&new.keys),
                        keys::describe(&new.action)
                    ),
                    Tone::Info,
                );
            }
        } else {
            self.refused = None;
            self.status("Not changed.", Tone::Info);
        }
        if self.pages.len() == 1 && SECTIONS[self.section] == "Key bindings" {
            // Back to the bindings view itself.
            self.close_page(false);
        }
    }

    /// Delete on a binding: an own one goes, a built-in one is released.
    pub(crate) fn key_remove(&mut self, i: usize) {
        let Some(r) = self.key_view.get(i).cloned() else {
            return;
        };
        if let KeySource::App(a) = &r.source {
            self.status(
                format!("{}: {a}'s own key; change it in that program.", r.label),
                Tone::Info,
            );
            return;
        }
        self.key_remove_keys(&r.keys);
    }

    fn key_remove_keys(&mut self, k: &str) {
        let Some(t) = self.keys_table() else { return };
        let items = keys::remove_binding(&self.user_keys(), &self.builtin_keys(), k);
        if self.commit_table(t, items, true) {
            trace(&format!("keyremoved|{k}"));
            self.rebuild();
            self.status(
                format!("{} removed · saving... · u undoes", keys::label(k)),
                Tone::Info,
            );
        }
        if self.pages.len() == 1 && SECTIONS[self.section] == "Key bindings" {
            self.close_page(false);
        }
    }

    /// r on a binding: NIRUWM's own binding on its keys again.
    pub(crate) fn key_restore(&mut self, k: &str) {
        let Some(t) = self.keys_table() else { return };
        let user = self.user_keys();
        if !user
            .iter()
            .filter_map(Binding::from_table)
            .any(|b| keys::same_keys(&b.keys, k))
        {
            self.status(
                format!("{} is NIRUWM's own already.", keys::label(k)),
                Tone::Info,
            );
            return;
        }
        let builtin = self
            .builtin_keys()
            .iter()
            .any(|d| keys::same_keys(&d.keys, k));
        if self.commit_table(t, keys::restore_default(&user, k), true) {
            trace(&format!("keyrestored|{k}"));
            self.rebuild();
            self.status(
                if builtin {
                    format!("{}: NIRUWM's own binding again · saving...", keys::label(k))
                } else {
                    format!(
                        "{} removed (NIRUWM has none there) · saving...",
                        keys::label(k)
                    )
                },
                Tone::Info,
            );
        }
    }

    /// A key combination pressed in the keys field: `super+shift+t`.
    pub fn captured(k: &KeyEv) -> Option<String> {
        let name = keys::keysym_name(k.base).or_else(|| keys::keysym_name(k.sym))?;
        let fkey =
            (0xffbe..0xffbe + 35).contains(&k.sym) || k.sym == 0xff61 || k.sym >= 0x1008_ff00;
        let editing = k.ctrl && !k.logo && !k.alt && matches!(k.ch, Some('u' | 'a' | 'e'));
        if editing || !(k.logo || k.alt || k.ctrl || fkey) {
            return None;
        }
        let mut parts: Vec<&str> = Vec::new();
        if k.logo {
            parts.push("super");
        }
        if k.ctrl {
            parts.push("ctrl");
        }
        if k.alt {
            parts.push("alt");
        }
        if k.shift {
            parts.push("shift");
        }
        let mut out = parts.join("+");
        if !out.is_empty() {
            out.push('+');
        }
        out.push_str(&name);
        Some(out)
    }

    // ── Preview ────────────────────────────────────────────────────────

    /// Run setting `e`'s preview command for `v` (or its current value).
    pub(crate) fn preview(&mut self, e: usize, v: Option<Value>) {
        let st = self.store.entries[e].setting.clone();
        if st.preview.is_empty() {
            return;
        }
        let Some(v) = v.or_else(|| self.current(e).value) else {
            return;
        };
        let value = match &v {
            Value::Array(a) => a.first().map(schema::plain).unwrap_or_default(),
            v => schema::plain(v),
        };
        let argv: Vec<String> = st
            .preview
            .iter()
            .map(|a| a.replace("{value}", &value))
            .collect();
        if paths_which(&argv[0]) {
            trace(&format!("preview|{}", argv.join(" ")));
            let _ = self.worker.tx.send(Req::Spawn(argv));
            self.status(
                format!("Preview of {value}: any key or click ends it."),
                Tone::Info,
            );
        } else {
            self.status(
                format!("{} is not installed: no preview.", argv[0]),
                Tone::Warn,
            );
        }
    }

    /// The help lines for the rows of the pages.
    pub(crate) fn page_help(&self, row: &Row) -> Option<(String, String)> {
        Some(match row {
            Row::Table(t) => {
                let tb = self.table(*t);
                let f = &self.store.files[self.store.tables[*t].file];
                (
                    tb.help.clone(),
                    format!(
                        "{} · [[{}]] in {} · Return opens the list",
                        self.store.schemas[self.store.tables[*t].schema].name, tb.key, f.rel
                    ),
                )
            }
            Row::Item(t, i) => {
                let item = self.store.items(*t).get(*i).cloned()?;
                let (a, b) = self.item_summary(*t, &item);
                let tb = self.table(*t);
                let moves = if tb.ordered {
                    " · Shift+Up/Down moves"
                } else {
                    ""
                };
                (
                    format!("{a}: {b}"),
                    format!(
                        "{} {} · Return edits · Delete removes{moves}",
                        tb.item,
                        i + 1
                    ),
                )
            }
            Row::Slot(t, n) => {
                let keys = format!(
                    "Super+Alt+{n} shows or hides it · Super+Alt+Shift+{n} sends the focused window there"
                );
                match self.slot_index(*t, *n) {
                    Some(i) => {
                        let item = self.store.items(*t)[i].clone();
                        let (a, b) = self.item_summary(*t, &item);
                        (
                            format!("Slot {n}: {a}, {b}"),
                            format!("{keys} · Return edits · Delete clears"),
                        )
                    }
                    None => (
                        format!("Slot {n} has no program: it holds the windows you send to it."),
                        format!("{keys} · Return gives it a program"),
                    ),
                }
            }
            Row::Field(k) => {
                let def = self.field_def(*k)?;
                let st = &def.setting;
                let f = self.form()?;
                let mut info = match (&st.default, def.required) {
                    (_, true) => "needed".to_string(),
                    (Some(d), false) => format!("default {}", short_value(st, d)),
                    (None, false) => "may be unset (Delete)".into(),
                };
                if def.offset != 0 {
                    info.push_str(&format!(
                        " · counted from {} here, written from {} in the file",
                        def.offset, 0
                    ));
                }
                if let Some(e) = &f.error {
                    info = format!("Not saved: {e}");
                } else if st.pattern.as_deref() == Some("keys") {
                    let (r, p) = self.key_conflicts();
                    if let Some(x) = r.or(p) {
                        info = x;
                    } else {
                        info.push_str(" · type them, or press the combination in the field");
                    }
                }
                (st.help.clone(), info)
            }
            Row::Class => {
                let st = self.class_setting();
                let info = match self.form_class() {
                    Some((c, Some(from))) => format!("{c}: {}", from.words()),
                    Some((c, None)) => format!("{c}: from the command"),
                    None => "none yet: choose a program, or type the class".into(),
                };
                (st.help, info)
            }
            Row::Released(j) => {
                let (label, was) = self.released_info(*j)?;
                (
                    format!("{label} is released: NIRUWM's {was} is off."),
                    "r or Delete puts NIRUWM's binding back".into(),
                )
            }
            Row::Key(i) => {
                let k = self.key_view.get(*i)?;
                let who = match &k.source {
                    KeySource::Builtin => "NIRUWM's own".to_string(),
                    KeySource::Yours => "yours".into(),
                    KeySource::Changed => "yours, in place of NIRUWM's (r puts it back)".into(),
                    KeySource::App(a) => format!("{a}'s key: change it there"),
                };
                let mut info = format!(
                    "{} · {who} · {}",
                    k.group,
                    k.action.as_deref().unwrap_or("a program's key")
                );
                if let Some(c) = &k.clash {
                    info = format!("{c} · {info}");
                }
                (format!("{}: {}", k.label, k.description), info)
            }
            Row::Act(Act::Pick, l) => (
                l.clone(),
                "Its window class is filled in as NIRUWM needs it.".into(),
            ),
            Row::Act(Act::Preview(e), l) => (
                l.clone(),
                format!(
                    "{} · any key or click ends the preview",
                    self.store.entries[*e].setting.preview.join(" ")
                ),
            ),
            Row::Act(Act::Save | Act::Cancel | Act::Remove, l) => (
                l.clone(),
                self.form()
                    .and_then(|f| f.error.clone())
                    .map(|e| format!("Not saved: {e}"))
                    .unwrap_or_else(|| {
                        "Checked by the schema and by NIRUWM before it is written.".into()
                    }),
            ),
            Row::Act(Act::Add(_) | Act::AddKey, l) => (
                l.clone(),
                "A form opens; nothing is written until Save.".into(),
            ),
            _ => return None,
        })
    }
}

fn paths_which(p: &str) -> bool {
    nirusettings::paths::which(p).is_some()
}
