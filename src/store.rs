//! The configuration files and the settings in them.
//!
//! Every file is kept twice: as a `toml_edit` document, which is what a
//! change edits (only the touched keys change; comments, order and
//! formatting stay), and as a plain `toml::Value` for reading. A write
//! always starts again from the file on disk, so a change made meanwhile by
//! someone else (NIRUWM saves layouts into its file) is kept.
use crate::paths;
use crate::schema::{self, Kind, Schema, Setting, Table};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item};

/// One configuration file.
pub struct File {
    /// Relative to the config home: `niruwm/config.toml`.
    pub rel: String,
    pub path: PathBuf,
    pub exists: bool,
    /// The text last read or written.
    pub text: String,
    pub value: toml::Value,
    /// Why the file cannot be used (it does not parse); its settings are
    /// then shown but never written.
    pub error: Option<String>,
    /// Indices of the schemas that describe this file.
    pub schemas: Vec<usize>,
}

/// Where the value shown comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Not in the file: the component's default.
    Default,
    /// Set in the file.
    File,
    /// Not in the file and no default: unset.
    Unset,
}

/// A setting's current state.
#[derive(Clone, Debug)]
pub struct Current {
    /// What the component uses.
    pub value: Option<toml::Value>,
    pub source: Source,
    /// The name used in the file when it is set (an alias, perhaps).
    pub name: Option<String>,
    /// The file's value fails the schema (shown with a warning).
    pub invalid: Option<String>,
}

/// One setting: its schema entry and its file.
pub struct Entry {
    pub schema: usize,
    pub file: usize,
    pub setting: Setting,
}

/// One list of tables: its schema entry and its file.
pub struct TableEntry {
    pub schema: usize,
    pub file: usize,
    pub table: Table,
}

pub struct Store {
    pub schemas: Vec<Schema>,
    pub files: Vec<File>,
    pub entries: Vec<Entry>,
    pub tables: Vec<TableEntry>,
}

/// A change to one file: keys (every name the key may have) and the new
/// value, `None` to remove the key.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub names: Vec<String>,
    pub value: Option<toml::Value>,
}

impl Store {
    pub fn new(schemas: Vec<Schema>) -> Store {
        let root = paths::config_home();
        let mut files: Vec<File> = Vec::new();
        let mut entries = Vec::new();
        let mut tables = Vec::new();
        for (si, s) in schemas.iter().enumerate() {
            let fi = match files.iter().position(|f| f.rel == s.file) {
                Some(i) => i,
                None => {
                    files.push(File {
                        rel: s.file.clone(),
                        path: root.join(&s.file),
                        exists: false,
                        text: String::new(),
                        value: toml::Value::Table(Default::default()),
                        error: None,
                        schemas: Vec::new(),
                    });
                    files.len() - 1
                }
            };
            files[fi].schemas.push(si);
            for st in &s.settings {
                entries.push(Entry {
                    schema: si,
                    file: fi,
                    setting: st.clone(),
                });
            }
            for t in &s.tables {
                tables.push(TableEntry {
                    schema: si,
                    file: fi,
                    table: t.clone(),
                });
            }
        }
        let mut store = Store {
            schemas,
            files,
            entries,
            tables,
        };
        for i in 0..store.files.len() {
            store.reload(i);
        }
        store
    }

    /// Read file `i` from disk again.
    pub fn reload(&mut self, i: usize) {
        let f = &mut self.files[i];
        match std::fs::read_to_string(&f.path) {
            Ok(t) => {
                f.exists = true;
                f.set_text(t);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                f.exists = false;
                f.text.clear();
                f.value = toml::Value::Table(Default::default());
                f.error = None;
            }
            Err(e) => {
                f.exists = true;
                f.error = Some(format!("cannot read {}: {e}", paths::tilde(&f.path)));
            }
        }
    }

    /// Re-read every file whose text on disk differs from what was read.
    /// Returns true when something changed.
    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        for i in 0..self.files.len() {
            let disk = std::fs::read_to_string(&self.files[i].path).ok();
            let same = match &disk {
                Some(t) => self.files[i].exists && *t == self.files[i].text,
                None => !self.files[i].exists,
            };
            if !same {
                self.reload(i);
                changed = true;
            }
        }
        changed
    }

    pub fn schema_of(&self, e: usize) -> &Schema {
        &self.schemas[self.entries[e].schema]
    }

    pub fn file_of(&self, e: usize) -> &File {
        &self.files[self.entries[e].file]
    }

    /// The state of entry `e`.
    pub fn current(&self, e: usize, dynamic: &[String]) -> Current {
        let entry = &self.entries[e];
        let st = &entry.setting;
        let f = &self.files[entry.file];
        for name in st.names() {
            if let Some(v) = lookup(&f.value, &name) {
                let invalid = crate::validate::value(st, v, dynamic).err();
                return Current {
                    value: Some(v.clone()),
                    source: Source::File,
                    name: Some(name),
                    invalid,
                };
            }
        }
        match &st.default {
            Some(d) => Current {
                value: Some(d.clone()),
                source: Source::Default,
                name: None,
                invalid: None,
            },
            None => Current {
                value: None,
                source: Source::Unset,
                name: None,
                invalid: None,
            },
        }
    }

    /// True when the value differs from the default.
    pub fn modified(&self, e: usize, cur: &Current) -> bool {
        let st = &self.entries[e].setting;
        match (&cur.value, &st.default) {
            (None, None) => false,
            (Some(v), Some(d)) => !same(v, d),
            _ => true,
        }
    }

    /// The changes that set entry `e` to `value` (with its `also_set`).
    pub fn changes_for(&self, e: usize, value: Option<toml::Value>) -> Vec<Change> {
        let st = &self.entries[e].setting;
        let mut out = vec![Change {
            names: st.names(),
            value,
        }];
        for (k, v) in &st.also_set {
            out.push(Change {
                names: vec![k.clone()],
                value: Some(v.clone()),
            });
        }
        out
    }

    /// Apply changes to file `fi` in memory (what the window shows at once;
    /// the writer applies the same changes to the file on disk).
    pub fn apply_local(&mut self, fi: usize, changes: &[Change]) -> Result<(), String> {
        let f = &mut self.files[fi];
        if let Some(e) = &f.error {
            return Err(e.clone());
        }
        let base = if f.exists || !f.text.is_empty() {
            f.text.clone()
        } else {
            self.schemas[f.schemas[0]].new_file.clone()
        };
        let text = edit_text(&base, changes)?;
        f.set_text(text);
        Ok(())
    }

    /// The items of list `t` as the file has them (stored numbers).
    pub fn items(&self, t: usize) -> Vec<toml::Table> {
        let te = &self.tables[t];
        let f = &self.files[te.file];
        lookup(&f.value, &te.table.key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_table().cloned()).collect())
            .unwrap_or_default()
    }

    /// The list `t` is in the file (possibly empty).
    pub fn table_in_file(&self, t: usize) -> bool {
        let te = &self.tables[t];
        lookup(&self.files[te.file].value, &te.table.key).is_some()
    }

    /// The change that makes list `t` hold `items` (none: the key goes).
    pub fn table_change(&self, t: usize, items: &[toml::Table]) -> Change {
        let key = self.tables[t].table.key.clone();
        Change {
            names: vec![key],
            value: (!items.is_empty()).then(|| {
                toml::Value::Array(
                    items
                        .iter()
                        .map(|i| toml::Value::Table(i.clone()))
                        .collect(),
                )
            }),
        }
    }

    /// The table of entry index `t` by its key: `scratchpad.slot`.
    pub fn table_by_key(&self, component: &str, key: &str) -> Option<usize> {
        self.tables
            .iter()
            .position(|te| te.table.key == key && self.schemas[te.schema].component == component)
    }

    /// The fixed and run-time choices of an enum, for display and cycling.
    pub fn choices(&self, e: usize, dynamic: &[String]) -> Vec<(toml::Value, String)> {
        let st = &self.entries[e].setting;
        if !st.choices.is_empty() {
            return st
                .choices
                .iter()
                .map(|c| (c.value.clone(), c.label.clone()))
                .collect();
        }
        dynamic
            .iter()
            .map(|d| (toml::Value::String(d.clone()), d.clone()))
            .collect()
    }
}

impl File {
    fn set_text(&mut self, t: String) {
        match t.parse::<toml::Value>() {
            Ok(v) => {
                self.value = v;
                self.error = t
                    .parse::<DocumentMut>()
                    .err()
                    .map(|e| format!("{}: {e}", self.rel));
            }
            Err(e) => {
                self.value = toml::Value::Table(Default::default());
                self.error = Some(format!(
                    "{} does not parse, so nothing in it is changed: {}",
                    paths::tilde(&self.path),
                    e.message()
                ));
            }
        }
        self.text = t;
    }
}

/// Values compare equal across int/float (`font_size = 15` is 15.0).
pub fn same(a: &toml::Value, b: &toml::Value) -> bool {
    use toml::Value::{Float, Integer};
    match (a, b) {
        (Integer(x), Float(y)) | (Float(y), Integer(x)) => (*x as f64 - y).abs() < 1e-9,
        (Float(x), Float(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// The value at a dotted key.
pub fn lookup<'a>(v: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut cur = v;
    for part in key.split('.') {
        cur = cur.as_table()?.get(part)?;
    }
    Some(cur)
}

/// Convert a value for `toml_edit`. Floats stay floats (`22.0`): some
/// components read a float key with `as_float` and ignore an integer.
pub fn to_edit(v: &toml::Value) -> toml_edit::Value {
    match v {
        toml::Value::String(s) => toml_edit::Value::from(s.as_str()),
        toml::Value::Integer(i) => toml_edit::Value::from(*i),
        toml::Value::Float(f) => toml_edit::Value::from(*f),
        toml::Value::Boolean(b) => toml_edit::Value::from(*b),
        toml::Value::Datetime(d) => d
            .to_string()
            .parse::<toml_edit::Value>()
            .unwrap_or_else(|_| toml_edit::Value::from(d.to_string())),
        toml::Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                arr.push(to_edit(x));
            }
            toml_edit::Value::Array(arr)
        }
        toml::Value::Table(t) => {
            let mut it = toml_edit::InlineTable::new();
            for (k, x) in t {
                it.insert(k, to_edit(x));
            }
            toml_edit::Value::InlineTable(it)
        }
    }
}

/// Apply `changes` to the TOML `text`, keeping everything else as it is.
pub fn edit_text(text: &str, changes: &[Change]) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("the file does not parse: {}", e.message()))?;
    // A file of comments only (a new file's header): toml_edit keeps those
    // comments as the document's trailing text, so new keys would land above
    // them. Edit an empty document and keep the comments on top instead.
    let header = if doc.as_table().is_empty() && !text.trim().is_empty() {
        doc = DocumentMut::new();
        let mut h = text.to_string();
        if !h.ends_with('\n') {
            h.push('\n');
        }
        Some(h)
    } else {
        None
    };
    for c in changes {
        edit_doc(&mut doc, c)?;
    }
    let out = match header {
        Some(h) if !doc.as_table().is_empty() => format!("{h}{doc}"),
        Some(h) => h,
        None => doc.to_string(),
    };
    // Paranoia: the result must parse, and hold what was asked.
    let v: toml::Value = out
        .parse()
        .map_err(|e: toml::de::Error| format!("internal error: {}", e.message()))?;
    for c in changes {
        let got = c.names.iter().find_map(|n| lookup(&v, n));
        match (&c.value, got) {
            (Some(want), Some(have)) if same(want, have) => {}
            (None, None) => {}
            _ => return Err(format!("internal error writing {}", c.names[0])),
        }
    }
    Ok(out)
}

fn edit_doc(doc: &mut DocumentMut, c: &Change) -> Result<(), String> {
    let (table_path, _) = schema::split_key(&c.names[0]);
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    if !table_path.is_empty() {
        for part in table_path.split('.') {
            if !table.contains_key(part) {
                if c.value.is_none() {
                    return Ok(());
                }
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                table.insert(part, Item::Table(t));
            }
            let item = table.get_mut(part).expect("just inserted");
            table = item
                .as_table_like_mut()
                .ok_or_else(|| format!("{part} in the file is not a table"))?;
        }
    }
    let leaves: Vec<&str> = c.names.iter().map(|n| schema::split_key(n).1).collect();
    let existing = leaves.iter().copied().find(|l| table.contains_key(l));
    if let Some(items) = c.value.as_ref().and_then(tables_of) {
        return edit_array_of_tables(table, existing.unwrap_or(leaves[0]), &items);
    }
    match (&c.value, existing) {
        (None, Some(l)) => {
            table.remove(l);
        }
        (None, None) => {}
        (Some(v), Some(l)) => {
            let item = table.get_mut(l).expect("present");
            let mut nv = to_edit(v);
            if let Some(old) = item.as_value() {
                // Keep a trailing comment and the spacing around the value.
                *nv.decor_mut() = old.decor().clone();
            } else {
                nv.decor_mut().set_prefix(" ");
            }
            *item = Item::Value(nv);
        }
        (Some(v), None) => {
            let mut nv = to_edit(v);
            nv.decor_mut().set_prefix(" ");
            table.insert(leaves[0], Item::Value(nv));
        }
    }
    Ok(())
}

/// The items of a value that is a non-empty list of tables.
fn tables_of(v: &toml::Value) -> Option<Vec<toml::Table>> {
    let a = v.as_array()?;
    if a.is_empty() {
        return None;
    }
    a.iter().map(|x| x.as_table().cloned()).collect()
}

/// A `toml_edit` value as a plain one.
fn plain_value(v: &toml_edit::Value) -> Option<toml::Value> {
    let mut d = DocumentMut::new();
    d.insert("x", Item::Value(v.clone()));
    let t: toml::Table = d.to_string().parse().ok()?;
    t.get("x").cloned()
}

/// A `toml_edit` table (one item of a list) as a plain one.
fn plain_table(t: &toml_edit::Table) -> toml::Table {
    let mut out = toml::Table::new();
    for (k, item) in t.iter() {
        if let Some(v) = item.as_value().and_then(plain_value) {
            out.insert(k.to_string(), v);
        }
    }
    out
}

/// Make the list of tables at `leaf` hold `items`. Items found unchanged in
/// the file are kept as they are, with their comments; a changed item is
/// edited key by key (its comments stay); new items go after the others.
/// The comments above the first item stay above the list.
fn edit_array_of_tables(
    table: &mut dyn toml_edit::TableLike,
    leaf: &str,
    items: &[toml::Table],
) -> Result<(), String> {
    let fresh = |item: &toml::Table| {
        let mut t = toml_edit::Table::new();
        for (k, v) in item {
            let mut nv = to_edit(v);
            nv.decor_mut().set_prefix(" ");
            t.insert(k, Item::Value(nv));
        }
        t.decor_mut().set_prefix("\n");
        t
    };
    let old: Vec<toml_edit::Table> = match table.get(leaf) {
        None => Vec::new(),
        Some(Item::ArrayOfTables(a)) => a.iter().cloned().collect(),
        Some(Item::Value(toml_edit::Value::Array(_))) => {
            // Written inline (`slot = [{ ... }]`): it stays inline.
            let item = table.get_mut(leaf).expect("present");
            let mut nv = to_edit(&toml::Value::Array(
                items.iter().cloned().map(toml::Value::Table).collect(),
            ));
            if let Some(o) = item.as_value() {
                *nv.decor_mut() = o.decor().clone();
            }
            *item = Item::Value(nv);
            return Ok(());
        }
        Some(_) => return Err(format!("{leaf} in the file is not a list of tables")),
    };
    let old_plain: Vec<toml::Table> = old.iter().map(plain_table).collect();
    // Which old item each new one is: the same content first, then the
    // remaining ones in order (an edited item keeps its place and comments).
    let mut used = vec![false; old.len()];
    let mut from: Vec<Option<usize>> = vec![None; items.len()];
    for (i, it) in items.iter().enumerate() {
        if let Some(j) = (0..old.len()).find(|&j| !used[j] && old_plain[j] == *it) {
            used[j] = true;
            from[i] = Some(j);
        }
    }
    for f in from.iter_mut() {
        if f.is_none() {
            if let Some(j) = (0..old.len()).find(|&j| !used[j]) {
                used[j] = true;
                *f = Some(j);
            }
        }
    }
    let mut positions: Vec<usize> = old.iter().filter_map(|t| t.position()).collect();
    positions.sort_unstable();
    let mut out = toml_edit::ArrayOfTables::new();
    for (i, it) in items.iter().enumerate() {
        let mut t = match from[i] {
            Some(j) => {
                let mut t = old[j].clone();
                let keys: Vec<String> = t.iter().map(|(k, _)| k.to_string()).collect();
                for k in keys {
                    if t.get(&k).is_some_and(|x| x.is_value()) && !it.contains_key(&k) {
                        t.remove(&k);
                    }
                }
                for (k, v) in it {
                    let same_now = t
                        .get(k)
                        .and_then(|x| x.as_value())
                        .and_then(plain_value)
                        .is_some_and(|o| same(&o, v));
                    if same_now {
                        continue;
                    }
                    let mut nv = to_edit(v);
                    match t.get(k).and_then(|x| x.as_value()) {
                        Some(o) => *nv.decor_mut() = o.decor().clone(),
                        None => nv.decor_mut().set_prefix(" "),
                    }
                    t.insert(k, Item::Value(nv));
                }
                t
            }
            None => fresh(it),
        };
        if let Some(p) = positions.get(i).or(positions.last()) {
            t.set_position(*p);
        }
        out.push(t);
    }
    // The comments above the list belong to the list, not to its first item.
    if let (Some(first_old), Some(first)) = (old.first(), out.get_mut(0)) {
        if from[0] != Some(0) {
            let lead = first_old.decor().prefix().cloned();
            if let Some(lead) = lead {
                first.decor_mut().set_prefix(lead);
            }
            for (i, f) in from.iter().enumerate().skip(1) {
                if *f == Some(0) {
                    if let Some(t) = out.get_mut(i) {
                        t.decor_mut().set_prefix("\n");
                    }
                }
            }
        }
    }
    table.insert(leaf, Item::ArrayOfTables(out));
    Ok(())
}

/// What a dynamic choice list holds.
pub fn dynamic_choices(name: &str) -> Vec<String> {
    match name {
        "themes" => crate::sources::themes(),
        _ => Vec::new(),
    }
}

/// True when a setting holds a number that +/- steps.
pub fn steps(st: &Setting) -> bool {
    matches!(st.kind, Kind::Int | Kind::Float)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_files_header_stays_above_the_first_key() {
        let header = "# NIRULOCK settings; written by NIRUSETTINGS.\n";
        let c = Change {
            names: vec!["layout".into()],
            value: Some(toml::Value::String("satie".into())),
        };
        let out = edit_text(header, &[c]).unwrap();
        assert!(out.starts_with(header), "{out}");
        assert!(out.ends_with("layout = \"satie\"\n"), "{out}");
    }
    use toml::Value;

    fn ch(name: &str, v: Option<Value>) -> Change {
        Change {
            names: vec![name.to_string()],
            value: v,
        }
    }

    const TEXT: &str = "# The theme.\ntheme = \"niru-noir\" # keep me\ngap = 8\n\n[bar]\n# Edge.\nposition = \"top\"\ntitle = false\n\n[[keys]]\nkeys = \"super+t\"\naction = \"layout tile\"\n";

    #[test]
    fn edits_keep_comments_order_and_other_keys() {
        let out = edit_text(
            TEXT,
            &[
                ch("gap", Some(Value::Integer(12))),
                ch("bar.position", Some(Value::String("bottom".into()))),
                ch("theme", Some(Value::String("niru-nord".into()))),
            ],
        )
        .unwrap();
        assert_eq!(
            out,
            TEXT.replace("gap = 8", "gap = 12")
                .replace("\"top\"", "\"bottom\"")
                .replace("niru-noir", "niru-nord")
        );
    }

    #[test]
    fn aliases_new_tables_floats_and_removal() {
        // `title` is an alias of show_title: the file's own name is edited,
        // never a second key next to it (that would be a duplicate field).
        let c = Change {
            names: vec!["bar.show_title".into(), "bar.title".into()],
            value: Some(Value::Boolean(true)),
        };
        let out = edit_text(TEXT, &[c]).unwrap();
        assert!(out.contains("title = true"), "{out}");
        assert!(!out.contains("show_title"));
        let out = edit_text(
            "font_size = 22\n",
            &[
                ch("font_size", Some(Value::Float(24.0))),
                ch("pause_music.enabled", Some(Value::Boolean(true))),
            ],
        )
        .unwrap();
        assert_eq!(out, "font_size = 24.0\n\n[pause_music]\nenabled = true\n");
        let out = edit_text(&out, &[ch("font_size", None), ch("missing.key", None)]).unwrap();
        assert_eq!(out, "\n[pause_music]\nenabled = true\n");
        let out = edit_text(
            "[sounds]\nenabled = true\n",
            &[ch("sounds.system.lock", Some(Value::Boolean(true)))],
        )
        .unwrap();
        assert_eq!(
            out,
            "[sounds]\nenabled = true\n\n[sounds.system]\nlock = true\n"
        );
        let out = edit_text(
            "",
            &[ch(
                "bar.modules",
                Some(Value::Array(vec![Value::String("clock".into())])),
            )],
        )
        .unwrap();
        assert_eq!(out, "[bar]\nmodules = [\"clock\"]\n");
    }

    fn items(v: &[&str]) -> Option<Value> {
        let t: Vec<Value> = v
            .iter()
            .map(|s| Value::Table(s.parse::<toml::Table>().unwrap()))
            .collect();
        (!t.is_empty()).then_some(Value::Array(t))
    }

    const RULES: &str = "gap = 8\n\n# Window rules: the first match wins.\n[[rules]]\nclass = \"a\" # first\nfloating = true\n\n# The mail program.\n[[rules]]\nclass = \"thunderbird\"\nworkspace = 7\n\n[bar]\nposition = \"top\"\n";

    #[test]
    fn lists_of_tables_keep_comments_and_untouched_items() {
        // Edit one field of the second rule: everything else stays.
        let out = edit_text(
            RULES,
            &[ch(
                "rules",
                items(&[
                    "class = 'a'\nfloating = true",
                    "class = 'thunderbird'\nworkspace = 2",
                ]),
            )],
        )
        .unwrap();
        assert_eq!(out, RULES.replace("workspace = 7", "workspace = 2"));
        // Add one: it goes after the others, before [bar].
        let out = edit_text(
            RULES,
            &[ch(
                "rules",
                items(&[
                    "class = 'a'\nfloating = true",
                    "class = 'thunderbird'\nworkspace = 7",
                    "class = 'gimp'",
                ]),
            )],
        )
        .unwrap();
        assert!(
            out.contains("workspace = 7\n\n[[rules]]\nclass = \"gimp\"\n\n[bar]"),
            "{out}"
        );
        // Remove the first: the comment above the list stays above it.
        let out = edit_text(
            RULES,
            &[ch(
                "rules",
                items(&["class = 'thunderbird'\nworkspace = 7"]),
            )],
        )
        .unwrap();
        assert!(
            out.contains(
                "# Window rules: the first match wins.\n[[rules]]\nclass = \"thunderbird\""
            ),
            "{out}"
        );
        assert!(!out.contains("# first"), "{out}");
        // Swap them: both keep their own lines.
        let out = edit_text(
            RULES,
            &[ch(
                "rules",
                items(&[
                    "class = 'thunderbird'\nworkspace = 7",
                    "class = 'a'\nfloating = true",
                ]),
            )],
        )
        .unwrap();
        let v: toml::Value = out.parse().unwrap();
        assert_eq!(v["rules"][0]["class"].as_str(), Some("thunderbird"));
        assert!(out.contains("class = \"a\" # first"), "{out}");
        assert!(out.find("[bar]").unwrap() > out.rfind("[[rules]]").unwrap());
        // Remove every one: the key goes.
        let out = edit_text(RULES, &[ch("rules", None)]).unwrap();
        assert!(!out.contains("[[rules]]"));
        assert!(out.contains("[bar]"));
    }

    #[test]
    fn a_new_list_goes_under_its_table() {
        let text = "[scratchpad]\ncommand = [\"kitty\", \"--class\", \"scratchpad\"]\n\n# Swallowing.\n[swallow]\nenabled = true\n";
        let out = edit_text(
            text,
            &[ch(
                "scratchpad.slot",
                items(&["number = 1\ncommand = ['kitty', '--class', 'scratch-btop']"]),
            )],
        )
        .unwrap();
        assert_eq!(
            out,
            "[scratchpad]\ncommand = [\"kitty\", \"--class\", \"scratchpad\"]\n\n[[scratchpad.slot]]\nnumber = 1\ncommand = [\"kitty\", \"--class\", \"scratch-btop\"]\n\n# Swallowing.\n[swallow]\nenabled = true\n"
        );
        // Written inline, it stays inline.
        let out = edit_text(
            "rules = [{ class = 'x' }]\n",
            &[ch("rules", items(&["class = 'y'"]))],
        )
        .unwrap();
        assert_eq!(out, "rules = [{ class = \"y\" }]\n");
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(edit_text("a = = 1", &[ch("a", Some(Value::Integer(1)))]).is_err());
        assert!(edit_text("bar = 1", &[ch("bar.x", Some(Value::Integer(1)))]).is_err());
    }
}
