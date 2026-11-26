//! Every schema key against the components' own example configurations:
//! the key must exist there (set, or named in a comment), a value set there
//! must pass the schema's checks, and the schema's default must be the
//! value the example sets, except where the example deliberately differs.
//! The fixtures are copies; checkouts of the component repositories next to
//! this one (`../niruwm`, ...) are read too when they exist.
use nirusettings::schema::{self, Schema};
use nirusettings::store::{lookup, same};
use std::path::Path;

/// Example files for a schema's config file: (fixture, sibling checkout).
fn examples(file: &str) -> Vec<(&'static str, &'static str)> {
    match file {
        "niruwm/config.toml" => vec![
            (
                "niruwm-config.example.toml",
                "../niruwm/config.example.toml",
            ),
            ("nirux-config.example.toml", "../nirux/config.example.toml"),
        ],
        "niruui/lock.toml" => vec![(
            "nirulock-config.example.toml",
            "../nirulock/config.example.toml",
        )],
        "niruui/screensaver.toml" => vec![(
            "niruscreensaver-config.example.toml",
            "../niruscreensaver/config.example.toml",
        )],
        "nirux/sounds.toml" => vec![("nirux-sounds.example.toml", "../nirux/sounds.example.toml")],
        "niruwm/appearance.toml" => vec![(
            "nirux-appearance.example.toml",
            "../nirux/appearance.example.toml",
        )],
        _ => vec![],
    }
}

/// Sources that define keys the examples only mention in prose.
fn sources(component: &str) -> &'static [&'static str] {
    match component {
        "niruwm" | "niruidle" => &["../niruwm/src/config.rs"],
        "nirubar" => &["../nirubar/src/config.rs"],
        "nirulock" => &["../nirulock/src/config.rs", "../nirulock/src/music.rs"],
        "niruscreensaver" => &["../niruscreensaver/src/config.rs"],
        _ => &[],
    }
}

/// Keys whose schema default is the component's built-in default while the
/// example sets another value on purpose.
const EXAMPLE_DIFFERS: &[&str] = &[
    // The code default is off; NIRUX's file turns the wallpaper on.
    "wallpaper.enabled",
    // The example uses Stockholm; the code default is "no location".
    "nightlight.latitude",
    "nightlight.longitude",
    // nirux keeps the shipped example's own list; the bar's built-in list
    // is older. The schema uses the NIRUX list.
    "bar.presenting_hide",
    // NIRUX installs its own programs; NIRUWM's built-in defaults are
    // kitty and brave. These settings cannot be reset (no_reset).
    "terminal",
    "terminal_alt",
    "browser",
    "scratchpad.command",
    // NIRUWM opens [[autostart.apps]] by default; NIRUX's file turns it off.
    "autostart.enabled",
];

/// Keys only the component's source names (no example mentions them); in
/// a run without checkouts they cannot be looked up.
const SOURCE_ONLY: &[&str] = &["bar.battery", "bar.network"];

/// Keys added by a concurrent release, not yet in any example (since).
fn is_new(st: &schema::Setting) -> bool {
    st.since.is_some()
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(p: &Path) -> Option<String> {
    // NIRUSETTINGS_FIXTURES_ONLY=1 runs as CI does, without checkouts.
    let sibling = p.components().any(|c| c.as_os_str() == "..");
    if sibling && std::env::var_os("NIRUSETTINGS_FIXTURES_ONLY").is_some() {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// Mentioned as a key: `name =` at the start of a (possibly commented)
/// line, or quoted as `` `name = ...` `` in a comment.
fn mentioned(text: &str, leaf: &str) -> bool {
    if text.contains(&format!("`{leaf} =")) {
        return true;
    }
    text.lines().any(|l| {
        let t = l.trim_start().trim_start_matches('#').trim_start();
        t.strip_prefix(leaf)
            .is_some_and(|r| r.trim_start().starts_with('='))
    })
}

fn check_schema(s: &Schema, problems: &mut Vec<String>) {
    let ex = examples(&s.file);
    let mut texts: Vec<(String, String)> = Vec::new();
    for (fixture, sibling) in &ex {
        let f = root().join("tests/fixtures").join(fixture);
        texts.push((fixture.to_string(), read(&f).expect("fixture")));
        if let Some(t) = read(&root().join(sibling)) {
            texts.push((sibling.to_string(), t));
        }
    }
    let src: String = sources(&s.component)
        .iter()
        .filter_map(|p| read(&root().join(p)))
        .collect();
    for st in &s.settings {
        let leaf = schema::split_key(&st.key).1;
        let names = st.names();
        let mut seen = false;
        for (name, text) in &texts {
            if mentioned(text, leaf) || st.aliases.iter().any(|a| mentioned(text, a)) {
                seen = true;
            }
            let value: toml::Value = match text.parse() {
                Ok(v) => v,
                Err(e) => {
                    // A sibling checkout may be mid-edit; the fixture must parse.
                    if name.starts_with("../") {
                        eprintln!("warning: {name} does not parse: {}", e.message());
                        continue;
                    }
                    panic!("{name}: {e}");
                }
            };
            let Some(v) = names.iter().find_map(|n| lookup(&value, n)) else {
                continue;
            };
            if let Err(e) = nirusettings::validate::value(st, v, &[]) {
                problems.push(format!("{}: {} = {v} in {name}: {e}", s.component, st.key));
            }
            if let Some(d) = &st.default {
                if !same(v, d) && !EXAMPLE_DIFFERS.contains(&st.key.as_str()) {
                    problems.push(format!(
                        "{}: {} default {d} but {name} sets {v}",
                        s.component, st.key
                    ));
                }
            }
        }
        if !seen && !src.is_empty() && src.contains(leaf) {
            seen = true;
        }
        if !seen && src.is_empty() && SOURCE_ONLY.contains(&st.key.as_str()) {
            seen = true;
        }
        if !seen && !is_new(st) && !texts.is_empty() {
            problems.push(format!(
                "{}: {} is not in any example{}",
                s.component,
                st.key,
                if src.is_empty() {
                    " (and no source checkout)"
                } else {
                    " or source"
                }
            ));
        }
    }
}

#[test]
fn every_schema_key_matches_the_examples() {
    let (schemas, warnings) = schema::load_all();
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut problems = Vec::new();
    for s in &schemas {
        check_schema(s, &mut problems);
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn bar_modules_match_the_bar_catalogue() {
    // NIRUBAR's own list of modules, when its source is next to this one.
    let Some(src) = read(&root().join("../nirubar/src/editor.rs")) else {
        return;
    };
    let (schemas, _) = schema::load_all();
    let st = schemas
        .iter()
        .flat_map(|s| &s.settings)
        .find(|s| s.key == "bar.modules")
        .unwrap();
    let start = src.find("pub const CATALOG").unwrap();
    let end = start + src[start..].find("];").unwrap();
    let cat = &src[start..end];
    let mut names = Vec::new();
    for part in cat.split('(').skip(1) {
        let t = part.trim_start();
        if let Some(rest) = t.strip_prefix('"') {
            names.push(rest.split('"').next().unwrap().to_string());
        }
    }
    let ours: Vec<String> = st.choices.iter().map(|c| schema::plain(&c.value)).collect();
    assert_eq!(ours, names);
}

/// NIRUBAR's own file has no example: every key must be read by the
/// NIRUBAR source that owns its table, and the panel names of
/// `[popup_timeout]` must be the bar's own list.
#[test]
fn nirubar_own_file_matches_the_bar() {
    let (schemas, _) = schema::load_all();
    let own = schemas
        .iter()
        .find(|s| s.file == "nirubar/config.toml")
        .expect("the nirubar/config.toml schema");
    let src = |f: &str| read(&root().join("../nirubar/src").join(f));
    let Some(timeout) = src("popup_timeout.rs") else {
        return;
    };
    let mut problems = Vec::new();
    for st in &own.settings {
        let (table, leaf) = schema::split_key(&st.key);
        let file = match table {
            "weather" => "weather.rs",
            "timer" => "timer.rs",
            "calendar" => "calendar.rs",
            "popup_timeout" => "popup_timeout.rs",
            other => {
                problems.push(format!("unknown table {other}"));
                continue;
            }
        };
        let text = src(file).unwrap_or_default();
        if table != "popup_timeout" && !text.contains(&format!("\"{leaf}\"")) {
            problems.push(format!("{}: {file} does not read {leaf}", st.key));
        }
    }
    // MODULES and EXTRA_NAMES, in the order the schema lists them.
    let names = |konst: &str| -> Vec<String> {
        let start = timeout.find(konst).unwrap();
        let end = start + timeout[start..].find("];").unwrap();
        timeout[start..end]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    };
    let mut bar = names("const MODULES");
    bar.extend(names("const EXTRA_NAMES"));
    let ours: Vec<String> = own
        .settings
        .iter()
        .filter_map(|s| s.key.strip_prefix("popup_timeout.").map(str::to_string))
        .collect();
    assert_eq!(ours, bar);
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// The fields of every list of tables against NIRUWM's structs: each field
/// is a field of the struct, and every struct field is in the schema.
#[test]
fn table_fields_match_niruwm() {
    let Some(src) = read(&root().join("../niruwm/src/config.rs")) else {
        return;
    };
    let fields_of = |strukt: &str| -> Vec<String> {
        let start = src.find(&format!("pub struct {strukt} {{")).unwrap();
        let end = start + src[start..].find("\n}").unwrap();
        src[start..end]
            .lines()
            .filter_map(|l| {
                let t = l.trim().strip_prefix("pub ")?;
                (!t.starts_with("struct"))
                    .then(|| t.split(':').next().unwrap_or("").trim().to_string())
            })
            .collect()
    };
    let (schemas, _) = schema::load_all();
    let wm = schemas.iter().find(|s| s.component == "niruwm").unwrap();
    for (key, strukt) in [
        ("scratchpad.slot", "ScratchpadSlot"),
        ("autostart.apps", "AutostartApp"),
        ("rules", "Rule"),
    ] {
        let t = wm.tables.iter().find(|t| t.key == key).unwrap();
        let mut ours: Vec<String> = t.fields.iter().map(|f| f.setting.key.clone()).collect();
        let mut theirs = fields_of(strukt);
        ours.sort();
        theirs.sort();
        assert_eq!(ours, theirs, "{key}");
    }
    let bindings = read(&root().join("../niruwm/src/bindings.rs")).unwrap_or_default();
    let start = bindings.find("pub struct RawBinding").unwrap();
    let end = start + bindings[start..].find("\n}").unwrap();
    for f in ["keys", "action", "description"] {
        assert!(bindings[start..end].contains(&format!("pub {f}:")), "{f}");
    }
    // Every action NIRUWM parses is one the window offers.
    let start = bindings.find("pub fn parse_action").unwrap();
    let end = start + bindings[start..].find("other => return Err").unwrap();
    for part in bindings[start..end].split("\n        \"").skip(1) {
        let verb = part.split('"').next().unwrap();
        let known = nirusettings::keys::ACTIONS
            .iter()
            .any(|(v, _, _)| *v == verb)
            || [
                "move_next",
                "move_prev",
                "rotatestack_next",
                "rotatestack_prev",
            ]
            .contains(&verb);
        assert!(known, "NIRUWM action {verb} is missing from keys::ACTIONS");
    }
}
