//! NIRUSETTINGS: a retro settings app for the NIRUX desktop.
mod app;
mod draw;
mod gui;
mod worker;

use nirusettings::{cli, schema, store::Store};

const HELP: &str = "NIRUSETTINGS, the settings of the NIRUX desktop

Usage:
  nirusettings [SECTION]       the window (one window: a second start raises it)
                               SECTION: appearance, wm, bar, lock, screensaver,
                               sound, keys, extras, updates, or 1 to 9
  nirusettings --list [SECTION] [--json]
                               every setting, its value and where it comes from
  nirusettings --get ID        one value (ID: component:key, or a key)
  nirusettings --set ID VALUE  check, write and apply one value
  nirusettings --reset ID      back to the default
  nirusettings --schemas       the schema files in use, and their problems
  nirusettings --version

In the window: arrows move, Left/Right or +/- change a value, Space
toggles, Return edits, r resets, u undoes, / searches, ? lists the keys.
The settings are described by schema files; see docs/SCHEMA.md.";

fn store() -> Store {
    let (schemas, warnings) = schema::load_all();
    for w in warnings {
        eprintln!("nirusettings: {w}");
    }
    Store::new(schemas)
}

fn fail(msg: &str) -> ! {
    eprintln!("nirusettings: {msg}");
    std::process::exit(1)
}

fn main() {
    let started = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut section = None;
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("NIRUSETTINGS {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return;
        }
        Some("--list") => {
            let json = args.iter().any(|a| a == "--json");
            let sec = args[1..].iter().find(|a| !a.starts_with("--"));
            let sec = sec.map(|s| {
                app::section_index(s)
                    .map(|i| schema::SECTIONS[i].to_string())
                    .unwrap_or_else(|| s.clone())
            });
            print!("{}", cli::list(&store(), sec.as_deref(), json));
            return;
        }
        Some("--get") => {
            let id = args.get(1).unwrap_or_else(|| fail("--get needs an ID"));
            match cli::get(&store(), id) {
                Ok(v) => println!("{v}"),
                Err(e) => fail(&e),
            }
            return;
        }
        Some("--set") => {
            let (Some(id), Some(v)) = (args.get(1), args.get(2)) else {
                fail("--set needs an ID and a value")
            };
            match cli::set(&mut store(), id, Some(v)) {
                Ok(m) => println!("{m}"),
                Err(e) => fail(&e),
            }
            return;
        }
        Some("--reset") => {
            let id = args.get(1).unwrap_or_else(|| fail("--reset needs an ID"));
            match cli::set(&mut store(), id, None) {
                Ok(m) => println!("{m}"),
                Err(e) => fail(&e),
            }
            return;
        }
        Some("--schemas") => {
            let (schemas, warnings) = schema::load_all();
            println!("override folder: {}", schema::override_dir().display());
            for s in &schemas {
                println!(
                    "{:<18} {:<26} {:>3} settings",
                    s.component,
                    s.file,
                    s.settings.len()
                );
            }
            for w in &warnings {
                println!("problem: {w}");
            }
            if !warnings.is_empty() {
                std::process::exit(1);
            }
            return;
        }
        Some(other) if other.starts_with('-') => {
            eprintln!("nirusettings: unknown option {other} (try --help)");
            std::process::exit(2);
        }
        Some(s) => match app::section_index(s) {
            Some(i) => section = Some(i),
            None => fail(&format!("no section {s} (try --help)")),
        },
        None => {}
    }
    const CLAIM: &str = "_NIRUSETTINGS_RUNNING";
    let what = section.map(|i| schema::SECTIONS[i]).unwrap_or("");
    let (c, screen_no) = match x11rb::connect(None) {
        Ok(c) => c,
        Err(e) => fail(&format!("cannot open the display: {e}")),
    };
    if std::env::var_os("NIRUSETTINGS_MULTI").is_none()
        && gui::x::raise_running(&c, screen_no, CLAIM, gui::shell::REQUEST_PROP, what)
    {
        if gui::shell::trace_on() {
            eprintln!("trace|instance|raised");
        }
        return;
    }
    let screen = gui::x::screen_of(&c, screen_no);
    let mut a = app::SettingsApp::new(section);
    if std::env::var_os("NIRUSETTINGS_TIMING").is_some() {
        eprintln!(
            "nirusettings: settings read after {:.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    let w = (screen.0 * 6 / 10).clamp(820, 1180) as u16;
    let h = (screen.1 * 7 / 10).clamp(560, 820) as u16;
    let opts = gui::shell::Opts {
        title: "NIRUSETTINGS",
        class: "nirusettings",
        size: (w, h),
        pos: ((screen.0 - w as i32) / 2, (screen.1 - h as i32) / 2),
        min: (720, 480),
        kind: gui::x::Kind::Normal,
        icon: gui::icons,
        watch: a.watch_dirs(),
        claim: Some(CLAIM),
        conn: Some((c, screen_no)),
    };
    if let Err(e) = gui::shell::run(&mut a, opts, started) {
        eprintln!("nirusettings: {e}");
        std::process::exit(1);
    }
}
