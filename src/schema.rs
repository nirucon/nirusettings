//! The schema files that describe every setting NIRUSETTINGS shows.
//!
//! One TOML file per component (`schema/niruwm.toml`, `schema/nirulock.toml`,
//! ...) names the configuration file, how a change is checked and applied,
//! and lists the settings with their key, type, default, allowed values and
//! a line of help. The window is generated from these files: a new setting
//! is a schema edit. The format is described in `docs/SCHEMA.md`, so other
//! programs (NIRULAUNCHER) can read the same files.
//!
//! The schemas are built into the binary. A file of the same name in
//! `$XDG_DATA_HOME/nirusettings/schema/` (or `$NIRUSETTINGS_SCHEMA_DIR`)
//! replaces the built-in one, so a schema can be updated without a rebuild.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The version of the schema format this program reads.
pub const FORMAT: i64 = 1;

/// The sections of the window, in order. Settings name one of the first
/// six; the last three are views of their own.
pub const SECTIONS: &[&str] = &[
    "Appearance",
    "Window manager",
    "Bar",
    "Lock screen",
    "Screensaver",
    "Sound",
    "Key bindings",
    "Extras",
    "Updates",
];

/// The built-in schema files, in the order their settings are listed.
pub const BUILTIN: &[(&str, &str)] = &[
    ("niruwm.toml", include_str!("../schema/niruwm.toml")),
    (
        "nirux-appearance.toml",
        include_str!("../schema/nirux-appearance.toml"),
    ),
    ("nirubar.toml", include_str!("../schema/nirubar.toml")),
    ("nirulock.toml", include_str!("../schema/nirulock.toml")),
    ("niruidle.toml", include_str!("../schema/niruidle.toml")),
    (
        "niruscreensaver.toml",
        include_str!("../schema/niruscreensaver.toml"),
    ),
    (
        "nirux-sounds.toml",
        include_str!("../schema/nirux-sounds.toml"),
    ),
];

/// The extras catalogue (descriptions for the Extras view).
pub const EXTRAS: &str = include_str!("../schema/extras.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Bool,
    Int,
    Float,
    Enum,
    String,
    Path,
    #[serde(alias = "color")]
    Colour,
    List,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Enum => "enum",
            Kind::String => "string",
            Kind::Path => "path",
            Kind::Colour => "colour",
            Kind::List => "list",
        }
    }
}

/// One allowed value of an enum (or of the items of a list).
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: toml::Value,
    pub label: String,
    pub help: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Full {
        value: toml::Value,
        #[serde(default)]
        label: String,
        #[serde(default)]
        help: String,
    },
    Plain(toml::Value),
}

fn choices<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Choice>, D::Error> {
    let raw: Vec<RawChoice> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|c| match c {
            RawChoice::Plain(v) => Choice {
                label: plain(&v),
                value: v,
                help: String::new(),
            },
            RawChoice::Full { value, label, help } => Choice {
                label: if label.is_empty() {
                    plain(&value)
                } else {
                    label
                },
                value,
                help,
            },
        })
        .collect())
}

/// A value as a person reads it: strings without quotes.
pub fn plain(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Float(f) => fmt_float(*f),
        toml::Value::Array(a) => a.iter().map(plain).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// A float with at most three decimals and at least one.
pub fn fmt_float(f: f64) -> String {
    let mut s = format!("{f:.3}");
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    s
}

/// How a list is typed in its text field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    /// Comma separated: `clock, photos, dark`.
    #[default]
    Comma,
    /// A command line: words, with "double quotes" around a word that holds
    /// spaces. The list is an argument list, never run by a shell.
    Words,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    /// The key path in the file, dotted: `bar.position`.
    pub key: String,
    /// Other names the component accepts for the same key (the file may use
    /// one of them; a change is written to the name the file already has).
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub label: String,
    #[serde(default)]
    pub help: String,
    pub section: String,
    #[serde(default)]
    pub group: String,
    /// The value the component uses when the key is absent. Absent here
    /// means "unset" is a value of its own (`optional` must be true).
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// The key may be absent; clearing the value removes it from the file.
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub unit: String,
    #[serde(default, deserialize_with = "choices")]
    pub choices: Vec<Choice>,
    /// Choices read from the system at run time (`themes`).
    #[serde(default)]
    pub choices_from: Option<String>,
    /// Lists: the type of each item (`string` or `enum`).
    #[serde(default)]
    pub item: Option<Kind>,
    /// Lists: every item at most once.
    #[serde(default)]
    pub unique: bool,
    /// Lists: the order matters (it is shown and can be changed).
    #[serde(default)]
    pub ordered: bool,
    #[serde(default)]
    pub min_items: Option<usize>,
    #[serde(default)]
    pub max_items: Option<usize>,
    #[serde(default)]
    pub split: Split,
    /// A named check of strings: `hhmm`, `hex`, `btaddr`, `strftime`,
    /// `theme`, `nonempty`, `word`.
    #[serde(default)]
    pub pattern: Option<String>,
    /// The apply mode for this setting (default: the file's `apply`).
    #[serde(default)]
    pub apply: Option<String>,
    /// The component version that first reads this key ("0.30").
    #[serde(default)]
    pub since: Option<String>,
    /// Other keys written together with this one, every time it changes.
    #[serde(default)]
    pub also_set: BTreeMap<String, toml::Value>,
    /// Shown, never changed here (with the reason in `help`).
    #[serde(default)]
    pub read_only: bool,
    /// No reset to the default (NIRUX writes a better value than the
    /// component's built-in default, such as its own terminal).
    #[serde(default)]
    pub no_reset: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    /// A command run after the file is written (argument list, no shell).
    #[serde(default)]
    pub run: Vec<String>,
    /// What the status line says once it is applied.
    pub says: String,
    /// True when the change is visible at once; false means it waits for a
    /// restart, the next lock, the next login...
    #[serde(default)]
    pub live: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub format: i64,
    /// A short id: `niruwm`.
    pub component: String,
    /// The name shown: `NIRUWM`.
    pub name: String,
    /// The configuration file, relative to `$XDG_CONFIG_HOME`.
    pub file: String,
    /// Written first when the file does not exist yet.
    #[serde(default)]
    pub new_file: String,
    /// The program that reads the file (for its version).
    #[serde(default)]
    pub binary: String,
    /// Commands that check a candidate file. They run with
    /// `XDG_CONFIG_HOME` pointing at a temporary tree that holds the
    /// candidate; exit status 0 means the component accepts it.
    #[serde(default)]
    pub check: Vec<Vec<String>>,
    /// Paths (relative to `$XDG_CONFIG_HOME`) linked into that tree, for
    /// checkers that read more than the one file.
    #[serde(default)]
    pub check_links: Vec<String>,
    /// The default apply mode.
    pub apply: String,
    #[serde(default)]
    pub modes: BTreeMap<String, Apply>,
    #[serde(default, rename = "setting")]
    pub settings: Vec<Setting>,
}

impl Setting {
    /// The item kind of a list (string when not given).
    pub fn item_kind(&self) -> Kind {
        self.item.unwrap_or(Kind::String)
    }

    /// The fixed choices as plain strings (for messages).
    pub fn choice_names(&self) -> String {
        self.choices
            .iter()
            .map(|c| plain(&c.value))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The leaf names this setting may be written as in the file: the key,
    /// then its aliases (same table).
    pub fn names(&self) -> Vec<String> {
        let mut out = vec![self.key.clone()];
        let (table, _) = split_key(&self.key);
        for a in &self.aliases {
            out.push(if table.is_empty() {
                a.clone()
            } else {
                format!("{table}.{a}")
            });
        }
        out
    }
}

/// `bar.position` -> ("bar", "position").
pub fn split_key(key: &str) -> (&str, &str) {
    match key.rfind('.') {
        Some(i) => (&key[..i], &key[i + 1..]),
        None => ("", key),
    }
}

/// Where replacement schema files are looked for.
pub fn override_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NIRUSETTINGS_SCHEMA_DIR") {
        return PathBuf::from(d);
    }
    crate::paths::data_home().join("nirusettings/schema")
}

/// Parse one schema file and check that it is consistent in itself.
pub fn parse(name: &str, text: &str) -> Result<Schema, String> {
    let s: Schema = toml::from_str(text).map_err(|e| format!("{name}: {e}"))?;
    check_schema(&s).map_err(|e| format!("{name}: {e}"))?;
    Ok(s)
}

fn check_schema(s: &Schema) -> Result<(), String> {
    if s.format != FORMAT {
        return Err(format!("format {} (this program reads {FORMAT})", s.format));
    }
    if !s.modes.contains_key(&s.apply) {
        return Err(format!(
            "apply mode {:?} is not defined in [modes]",
            s.apply
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for st in &s.settings {
        let at = |m: &str| format!("setting {}: {m}", st.key);
        if !seen.insert(st.key.clone()) {
            return Err(at("listed twice"));
        }
        if !SECTIONS[..6].contains(&st.section.as_str()) {
            return Err(at(&format!("unknown section {:?}", st.section)));
        }
        if let Some(m) = &st.apply {
            if !s.modes.contains_key(m) {
                return Err(at(&format!("apply mode {m:?} is not defined")));
            }
        }
        if st.default.is_none() && !st.optional {
            return Err(at("needs a default (or optional = true)"));
        }
        if st.kind == Kind::Enum && st.choices.is_empty() && st.choices_from.is_none() {
            return Err(at("an enum needs choices"));
        }
        if st.kind == Kind::List && st.item_kind() == Kind::Enum && st.choices.is_empty() {
            return Err(at("a list of enum items needs choices"));
        }
        if let Some(d) = &st.default {
            // The default must pass its own checks (dynamic choices aside).
            if st.choices_from.is_none() {
                crate::validate::value(st, d, &[]).map_err(|e| at(&format!("default: {e}")))?;
            }
        }
        if !st.choices.is_empty() && !matches!(st.kind, Kind::Enum | Kind::List) {
            return Err(at(&format!("choices on a {} setting", st.kind.name())));
        }
    }
    Ok(())
}

/// Every schema: the built-in ones, each replaced by a file of the same
/// name in the override directory when that file parses. Problems with an
/// override are returned as warnings and the built-in schema is kept.
pub fn load_all() -> (Vec<Schema>, Vec<String>) {
    let dir = override_dir();
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for (name, text) in BUILTIN {
        let over = std::fs::read_to_string(dir.join(name)).ok();
        if let Some(t) = over {
            match parse(name, &t) {
                Ok(s) => {
                    out.push(s);
                    continue;
                }
                Err(e) => warnings.push(format!("{} (built-in schema used)", e)),
            }
        }
        match parse(name, text) {
            Ok(s) => out.push(s),
            Err(e) => warnings.push(e),
        }
    }
    (out, warnings)
}

#[derive(Clone, Debug, Deserialize)]
pub struct Extra {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Deserialize)]
struct ExtrasFile {
    #[serde(rename = "extra")]
    extras: Vec<Extra>,
}

pub fn extras() -> Vec<Extra> {
    toml::from_str::<ExtrasFile>(EXTRAS)
        .map(|f| f.extras)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_schema_parses() {
        for (name, text) in BUILTIN {
            let s = parse(name, text).unwrap_or_else(|e| panic!("{e}"));
            assert!(!s.settings.is_empty(), "{name} has no settings");
        }
        assert!(extras().len() >= 10);
    }

    #[test]
    fn split_and_aliases() {
        assert_eq!(split_key("bar.position"), ("bar", "position"));
        assert_eq!(split_key("gap"), ("", "gap"));
        let (all, w) = load_all();
        assert!(w.is_empty(), "{w:?}");
        let title = all
            .iter()
            .flat_map(|s| &s.settings)
            .find(|s| s.key == "bar.show_title")
            .unwrap();
        assert_eq!(title.names(), vec!["bar.show_title", "bar.title"]);
        assert_eq!(fmt_float(0.55), "0.55");
        assert_eq!(fmt_float(15.0), "15.0");
    }
}
