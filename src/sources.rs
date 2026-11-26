//! What NIRUSETTINGS asks the system: installed themes, program versions,
//! the key bindings in effect, the extras. Every command runs off the
//! window's thread (in the worker) with a time limit.
use crate::paths;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The themes built into NIRUWM.
pub const BUILTIN_THEMES: &[&str] = &[
    "niru-noir",
    "niru-nord",
    "niru-satie",
    "niru-carl-larsson",
    "niru-forest-vigil",
    "niru-blood-of-urdr",
    "niru-root-and-stone",
    "niru-blue-horizon",
    "niru-the-norns",
    "niru-winter-nights",
];

/// Every theme NIRUWM can load: the built-in ones, then the files in
/// `~/.config/niruwm/themes`.
pub fn themes() -> Vec<String> {
    let mut out: Vec<String> = BUILTIN_THEMES.iter().map(|s| s.to_string()).collect();
    let dir = paths::config_home().join("niruwm/themes");
    let mut extra: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let stem = n.strip_suffix(".toml")?.to_string();
            crate::validate::pattern("theme", &stem).ok()?;
            Some(stem)
        })
        .filter(|n| !out.contains(n))
        .collect();
    extra.sort();
    out.extend(extra);
    out
}

/// The output of a finished command.
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run `argv` with a time limit; `None` when it cannot start or runs over.
pub fn run(argv: &[String], env: &[(&str, &std::ffi::OsStr)], limit: Duration) -> Option<Output> {
    let (prog, args) = argv.split_first()?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let t_out = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let so = t_out.join().unwrap_or_default();
    let se = t_err.join().unwrap_or_default();
    Some(Output {
        ok: status.success(),
        stdout: String::from_utf8_lossy(&so).into_owned(),
        stderr: String::from_utf8_lossy(&se).into_owned(),
    })
}

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// The version a program prints for `--version` ("NIRUWM 0.37.15").
pub fn version(binary: &str) -> Option<String> {
    paths::which(binary)?;
    let o = run(&argv(&[binary, "--version"]), &[], Duration::from_secs(3))?;
    let text = if o.stdout.trim().is_empty() {
        o.stderr
    } else {
        o.stdout
    };
    parse_version(&text)
}

/// The first word that looks like a version number.
pub fn parse_version(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .map(|w| w.trim_start_matches('v'))
        .find(|w| {
            w.contains('.')
                && w.chars().next().is_some_and(|c| c.is_ascii_digit())
                && w.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(str::to_string)
}

/// `a` is older than `b` ("0.29.0" < "0.30").
pub fn older(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u64> { s.split('.').map(|x| x.parse().unwrap_or(0)).collect() };
    let (x, y) = (p(a), p(b));
    for i in 0..x.len().max(y.len()) {
        let (u, v) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if u != v {
            return u < v;
        }
    }
    false
}

/// One key binding as the cheat sheet shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub keys: String,
    pub label: String,
    pub description: String,
    pub group: String,
    pub source: String,
    /// The NIRUWM action (`layout tile`); none for a key a program owns.
    pub action: Option<String>,
}

/// The bindings in effect: `niructl keys --json`, or the `[[keys]]` of the
/// NIRUWM file when niructl is not there.
pub fn keys(config: &toml::Value) -> (Vec<Key>, String) {
    if paths::which("niructl").is_some() {
        if let Some(o) = run(
            &argv(&["niructl", "keys", "--json"]),
            &[],
            Duration::from_secs(5),
        ) {
            if o.ok {
                if let Some(k) = parse_keys_json(&o.stdout) {
                    return (k, "niructl keys --json".into());
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Some(list) = config.get("keys").and_then(|k| k.as_array()) {
        for k in list {
            let keys = k.get("keys").and_then(|v| v.as_str()).unwrap_or("");
            let action = k.get("action").and_then(|v| v.as_str()).unwrap_or("");
            if keys.is_empty() || action == "unbind" {
                continue;
            }
            let desc = k
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| describe(action));
            out.push(Key {
                keys: keys.to_string(),
                label: key_label(keys),
                description: desc,
                group: group_of(action).to_string(),
                source: "config".into(),
                action: Some(action.to_string()),
            });
        }
    }
    (out, "[[keys]] in niruwm/config.toml".into())
}

pub fn parse_keys_json(text: &str) -> Option<Vec<Key>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let entries = v.get("entries")?.as_array()?;
    let s = |e: &serde_json::Value, k: &str| {
        e.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    Some(
        entries
            .iter()
            .map(|e| {
                let keys = s(e, "keys");
                let label = match s(e, "label") {
                    l if l.is_empty() => key_label(&keys),
                    l => l,
                };
                Key {
                    label,
                    keys,
                    description: s(e, "description"),
                    group: s(e, "group"),
                    source: s(e, "source"),
                    action: e.get("action").and_then(|a| a.as_str()).map(str::to_string),
                }
            })
            .collect(),
    )
}

/// NIRUWM's built-in bindings: what `niruwm --dump-config` prints for a
/// configuration folder without a file. None when niruwm is not installed.
pub fn default_keys() -> Option<Vec<crate::keys::Binding>> {
    paths::which("niruwm")?;
    let empty = paths::cache_dir().join(format!("defaults-{}", std::process::id()));
    let _ = std::fs::create_dir_all(empty.join("niruwm"));
    let o = run(
        &argv(&["niruwm", "--dump-config"]),
        &[("XDG_CONFIG_HOME", empty.as_os_str())],
        Duration::from_secs(5),
    );
    let _ = std::fs::remove_dir_all(&empty);
    let o = o.filter(|o| o.ok)?;
    let v: toml::Value = o.stdout.parse().ok()?;
    let list: Vec<crate::keys::Binding> = v
        .get("keys")?
        .as_array()?
        .iter()
        .filter_map(|t| t.as_table().and_then(crate::keys::Binding::from_table))
        .collect();
    (!list.is_empty()).then_some(list)
}

/// `super+shift+Return` -> `Super+Shift+Return`.
pub fn key_label(spec: &str) -> String {
    spec.split('+')
        .map(|p| match p {
            "super" => "Super".to_string(),
            "shift" => "Shift".to_string(),
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            p if p.len() == 1 => p.to_ascii_uppercase(),
            p => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// An action as words: "layout tile" -> "Layout: tile".
fn describe(action: &str) -> String {
    let mut it = action.splitn(2, ' ');
    let verb = it.next().unwrap_or("").replace('_', " ");
    let mut out: String = verb
        .chars()
        .enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    if let Some(rest) = it.next() {
        out.push_str(": ");
        out.push_str(rest);
    }
    out
}

pub fn group_of(action: &str) -> &'static str {
    let w = action.split_whitespace().next().unwrap_or("");
    match w {
        "tag" | "movetotag" | "toggletag" | "togglemovetotag" | "tag_next" | "tag_prev" => {
            "Workspaces"
        }
        "layout" | "layout_next" | "layout_prev" | "inc_master" | "dec_master"
        | "inc_master_ratio" | "dec_master_ratio" | "scroll_columns" => "Layouts",
        "focus_next" | "focus_prev" | "move_next" | "move_prev" | "focus_urgent" | "close"
        | "toggle_float" | "toggle_fullscreen" | "toggle_gaps" | "move_float" | "resize_float"
        | "swap_next" | "swap_prev" => "Windows",
        "start" | "shell" | "terminal" | "terminal_alt" | "browser" => "Programs",
        w if w.starts_with("launcher") || w == "overview" || w == "window_switcher" => "Launcher",
        _ => "Session",
    }
}

/// One extra as `nirux extras list` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub name: String,
    pub pinned: String,
    pub installed: Option<String>,
    pub selected: bool,
    pub description: String,
    pub status: String,
}

/// The extras: the catalogue merged with `nirux extras list`. The second
/// value says where the state comes from (or why it is missing).
pub fn extras() -> (Vec<Extra>, String) {
    let cat = crate::schema::extras();
    let mut out: Vec<Extra> = cat
        .iter()
        .map(|c| Extra {
            name: c.name.clone(),
            pinned: String::new(),
            installed: None,
            selected: false,
            description: c.description.clone(),
            status: c.status.clone(),
        })
        .collect();
    if paths::which("nirux").is_none() {
        return (out, "nirux is not installed: install state unknown".into());
    }
    let Some(o) = run(
        &argv(&["nirux", "extras", "list"]),
        &[],
        Duration::from_secs(20),
    ) else {
        return (out, "nirux extras list did not answer".into());
    };
    if !o.ok {
        return (out, "nirux extras list failed".into());
    }
    merge_extras(&mut out, &o.stdout);
    (out, "nirux extras list".into())
}

/// Lay `nirux extras list` (NAME PINNED INSTALLED SELECTED) over the list.
pub fn merge_extras(out: &mut Vec<Extra>, text: &str) {
    for line in text.lines().skip(1) {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.len() < 4 {
            continue;
        }
        let installed = (w[2] != "-").then(|| w[2].to_string());
        let selected = w[3] == "yes";
        match out.iter_mut().find(|e| e.name == w[0]) {
            Some(e) => {
                e.pinned = w[1].to_string();
                e.installed = installed;
                e.selected = selected;
            }
            None => out.push(Extra {
                name: w[0].to_string(),
                pinned: w[1].to_string(),
                installed,
                selected,
                description: String::new(),
                status: String::new(),
            }),
        }
    }
}

/// The installed NIRUX release and the newest one the bar has seen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nirux {
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub components: Vec<(String, Option<String>)>,
}

pub fn nirux() -> Nirux {
    let comps = [
        "niruwm",
        "nirubar",
        "nirulauncher",
        "nirulock",
        "niruscreensaver",
        "nirunotify",
        "nirusettings",
    ];
    Nirux {
        installed: version("nirux"),
        latest: latest_seen(),
        components: comps.iter().map(|c| (c.to_string(), version(c))).collect(),
    }
}

/// NIRUBAR's daily check keeps the newest release in its cache.
fn latest_seen() -> Option<String> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths::home().join(".cache"))
        .join("nirubar");
    for name in ["nirux-latest", "niruxupdate", "nirux-update"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(name)) {
            if let Some(v) = parse_version(&t) {
                return Some(v);
            }
        }
    }
    None
}

/// A terminal command line that runs `script` and waits for Enter, the
/// way NIRUBAR opens `nirux clean` (kitty takes the program as its trailing
/// arguments, the others want -e).
pub fn terminal_command(terminal: &[String], script: &str) -> Option<Vec<String>> {
    let (program, rest) = terminal.split_first()?;
    let mut out = vec![program.clone()];
    out.extend(rest.iter().cloned());
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    let has_exec = rest.iter().any(|a| a == "-e" || a == "--");
    if name != "kitty" && !has_exec {
        out.push("-e".into());
    }
    out.extend([
        "sh".into(),
        "-c".into(),
        format!("{script}; printf '\\nPress Enter to close. '; read _"),
    ]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert_eq!(
            parse_version("NIRUWM 0.37.15\n").as_deref(),
            Some("0.37.15")
        );
        assert_eq!(parse_version("nirulock v0.29.0").as_deref(), Some("0.29.0"));
        assert_eq!(parse_version("no version"), None);
        assert!(older("0.29.0", "0.30"));
        assert!(!older("0.30.0", "0.30"));
        assert!(!older("1.0", "0.30"));
    }

    #[test]
    fn extras_list_merges() {
        let mut out = vec![Extra {
            name: "nirusys".into(),
            pinned: String::new(),
            installed: None,
            selected: false,
            description: "monitor".into(),
            status: "alpha".into(),
        }];
        merge_extras(
            &mut out,
            "NAME         PINNED    INSTALLED    SELECTED\nnirusys      0.1.0     0.1.0        yes\nnplay        2.0.1     -            no\n",
        );
        assert_eq!(out[0].installed.as_deref(), Some("0.1.0"));
        assert!(out[0].selected);
        assert_eq!(out[1].name, "nplay");
        assert_eq!(out[1].installed, None);
    }

    #[test]
    fn keys_from_json_and_terminals() {
        let k = parse_keys_json(r#"{"entries":[{"keys":"super+t","label":"Super+T","description":"Tile","group":"Layouts","source":"default"}]}"#).unwrap();
        assert_eq!(k[0].label, "Super+T");
        assert_eq!(key_label("super+shift+Return"), "Super+Shift+Return");
        let t = terminal_command(&["kitty".into()], "nirux update").unwrap();
        assert_eq!(t[1], "sh");
        let t = terminal_command(&["alacritty".into()], "x").unwrap();
        assert_eq!(t[1], "-e");
    }
}
