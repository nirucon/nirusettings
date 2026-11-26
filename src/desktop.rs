//! The installed desktop programs (the `.desktop` files of the XDG data
//! folders) and the window class NIRUWM needs to recognise one: a
//! scratchpad, an autostart entry or a window rule names its program's
//! window by its class.
//!
//! NIRUWM reads a scratchpad's class from the command itself: the value of
//! `--class`, `-class`, `--app-id`, `--name` or `-name`. A program that has
//! no such option is started as `sh -c "exec PROGRAM" --class CLASS`: the
//! shell ignores the last two words, NIRUWM reads them, and CLASS must be
//! the program's real `WM_CLASS`. That class comes from the desktop file's
//! `StartupWMClass`, from the list of NIRUX programs below, or, failing
//! both, is a guess the person is asked to check.
use crate::paths;
use std::path::{Path, PathBuf};

/// One installed program.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// The desktop file's name without `.desktop`: `nirufm`.
    pub id: String,
    pub name: String,
    pub comment: String,
    /// `Exec` as an argument list, field codes (`%U`) removed.
    pub exec: Vec<String>,
    pub icon: String,
    pub wm_class: Option<String>,
    /// Runs in a terminal (`Terminal=true`).
    pub terminal: bool,
    pub path: PathBuf,
}

/// Where a window class comes from, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassFrom {
    /// The command names it (`--class NAME`).
    Command,
    /// `StartupWMClass` in the desktop file.
    Desktop,
    /// A NIRUX program: its class is its own name.
    Nirux,
    /// The program's name: probably right, to be checked.
    Guess,
}

impl ClassFrom {
    pub fn words(self) -> &'static str {
        match self {
            ClassFrom::Command => "from the command",
            ClassFrom::Desktop => "from its desktop file",
            ClassFrom::Nirux => "a NIRUX program's own",
            ClassFrom::Guess => "a guess: check it with xprop WM_CLASS",
        }
    }
}

/// The NIRUX programs: each one's window class is its own name.
pub const NIRUX_PROGRAMS: &[&str] = &[
    "niruamp",
    "niruclip",
    "nirudemo",
    "nirudictate",
    "nirudisplay",
    "nirueditview",
    "niruflow",
    "nirufm",
    "nirugame",
    "niruio",
    "nirulauncher",
    "nirunote",
    "nirupres",
    "niruprodrive",
    "nirusec",
    "nirusettings",
    "nirusid",
    "nirusys",
    "niruterm",
    "niruthemeselector",
    "nirutoday",
    "niruwallpaper",
    "nplay",
];

/// Programs that give their window the class they are told, and how.
pub fn class_flag(program: &str) -> Option<&'static str> {
    let name = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    Some(match name {
        "niruterm" | "kitty" | "alacritty" | "niruamp" => "--class",
        "foot" => "--app-id",
        "xterm" | "uxterm" => "-class",
        "urxvt" | "rxvt" => "-name",
        _ => return None,
    })
}

fn is_class_flag(arg: &str) -> bool {
    matches!(arg, "--class" | "-class" | "--app-id" | "-name" | "--name")
}

/// The class a command gives its window, read as NIRUWM reads it.
pub fn command_class(command: &[String]) -> Option<String> {
    let mut args = command.iter().peekable();
    while let Some(arg) = args.next() {
        if let Some((flag, value)) = arg.split_once('=') {
            if is_class_flag(flag) && !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if is_class_flag(arg) {
            if let Some(name) = args.peek().filter(|n| !n.is_empty()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// The program a command runs: the first word, or what `sh -c "exec X"`
/// starts.
pub fn program_of(command: &[String]) -> Option<String> {
    let first = command.first()?;
    if (first == "sh" || first == "bash") && command.get(1).map(String::as_str) == Some("-c") {
        let script = command.get(2)?;
        let words = crate::validate::words(script.trim_start_matches("exec ")).ok()?;
        return words.first().map(|w| base(w));
    }
    // A terminal running a program: the program (`niruterm -e btop`).
    if class_flag(first).is_some() {
        if let Some(i) = command.iter().position(|w| w == "-e" || w == "--") {
            if let Some(p) = command.get(i + 1) {
                return Some(base(p));
            }
        }
    }
    Some(base(first))
}

fn base(p: &str) -> String {
    Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(p)
        .to_string()
}

/// The window class of a program, best guess first, with its source.
pub fn class_for(command: &[String], app: Option<&App>) -> Option<(String, ClassFrom)> {
    if let Some(c) = command_class(command) {
        return Some((c, ClassFrom::Command));
    }
    if let Some(c) = app.and_then(|a| a.wm_class.clone()) {
        return Some((c, ClassFrom::Desktop));
    }
    let prog = program_of(command)?;
    if NIRUX_PROGRAMS.contains(&prog.as_str()) {
        return Some((prog, ClassFrom::Nirux));
    }
    let lower = prog.to_lowercase();
    (!lower.is_empty()).then_some((lower, ClassFrom::Guess))
}

/// Quote one word for `sh`.
pub fn sh_quote(w: &str) -> String {
    if !w.is_empty()
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c))
    {
        w.to_string()
    } else {
        format!("'{}'", w.replace('\'', "'\\''"))
    }
}

/// A command whose window NIRUWM recognises by `class`: the program's own
/// class option when it has one, else `sh -c "exec PROGRAM" --class CLASS`.
/// A command that already names a class gets `class` in its place.
pub fn with_class(command: &[String], class: &str) -> Vec<String> {
    // Replace the class a command already names.
    let mut out = command.to_vec();
    let mut i = 0;
    while i < out.len() {
        if let Some((flag, _)) = out[i].split_once('=') {
            if is_class_flag(flag) {
                out[i] = format!("{flag}={class}");
                return out;
            }
        }
        if is_class_flag(&out[i]) && i + 1 < out.len() {
            out[i + 1] = class.to_string();
            return out;
        }
        i += 1;
    }
    let Some(first) = command.first() else {
        return Vec::new();
    };
    if let Some(flag) = class_flag(first) {
        let mut out = vec![first.clone(), flag.to_string(), class.to_string()];
        out.extend(command[1..].iter().cloned());
        return out;
    }
    let script = format!(
        "exec {}",
        command
            .iter()
            .map(|w| sh_quote(w))
            .collect::<Vec<_>>()
            .join(" ")
    );
    vec![
        "sh".into(),
        "-c".into(),
        script,
        "--class".into(),
        class.to_string(),
    ]
}

/// The command a person reads: `sh -c "exec nirufm" --class nirufm` shows
/// as `nirufm`.
pub fn shown_command(command: &[String]) -> String {
    if command.len() == 5
        && command[0] == "sh"
        && command[1] == "-c"
        && command[2].starts_with("exec ")
        && is_class_flag(&command[3])
    {
        return command[2]["exec ".len()..].to_string();
    }
    crate::validate::join_words(command)
}

/// A scratchpad command for a program: a terminal program (`btop`) runs in
/// `terminal`, given the class `scratch-NAME`; a program with a class
/// option gets the same; any other program keeps its own class.
pub fn scratchpad_command(
    app: &App,
    terminal: &[String],
) -> (Vec<String>, Option<(String, ClassFrom)>) {
    let short: String = app
        .id
        .rsplit('.')
        .next()
        .unwrap_or(&app.id)
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let own = format!("scratch-{short}");
    if app.terminal {
        let term = if terminal.is_empty() {
            vec!["niruterm".to_string()]
        } else {
            terminal.to_vec()
        };
        if term.first().and_then(|t| class_flag(t)).is_some() {
            let mut cmd = with_class(&term, &own);
            cmd.push("-e".into());
            cmd.extend(app.exec.iter().cloned());
            return (cmd, Some((own, ClassFrom::Command)));
        }
    }
    if app.exec.first().and_then(|p| class_flag(p)).is_some() {
        return (with_class(&app.exec, &own), Some((own, ClassFrom::Command)));
    }
    match class_for(&app.exec, Some(app)) {
        Some((class, from)) => (with_class(&app.exec, &class), Some((class, from))),
        None => (app.exec.clone(), None),
    }
}

/// The XDG data folders that hold `applications/`, most important first.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut out = vec![paths::data_home()];
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        let p = PathBuf::from(d);
        if p.is_absolute() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Every program a launcher would show, sorted by name. A desktop file in
/// an earlier folder hides one of the same name in a later one.
pub fn installed() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for d in data_dirs() {
        let mut files = Vec::new();
        collect(&d.join("applications"), "", &mut files, 0);
        files.sort();
        for (id, path) in files {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| parse(&id, &path, &t))
            {
                out.push(app);
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>, depth: u32) {
    if depth > 3 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if p.is_dir() {
            collect(&p, &format!("{prefix}{name}-"), out, depth + 1);
        } else if let Some(stem) = name.strip_suffix(".desktop") {
            out.push((format!("{prefix}{stem}"), p));
        }
    }
}

/// One desktop file's `[Desktop Entry]`, when it is a program a launcher
/// shows.
pub fn parse(id: &str, path: &Path, text: &str) -> Option<App> {
    let mut in_entry = false;
    let mut get = std::collections::HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_entry = l == "[Desktop Entry]";
            continue;
        }
        if !in_entry || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            get.entry(k.trim().to_string())
                .or_insert_with(|| v.trim().to_string());
        }
    }
    let val = |k: &str| get.get(k).cloned().unwrap_or_default();
    if val("Type") != "Application"
        || val("NoDisplay") == "true"
        || val("Hidden") == "true"
        || val("Name").is_empty()
    {
        return None;
    }
    let try_exec = val("TryExec");
    if !try_exec.is_empty() && paths::which(&try_exec).is_none() {
        return None;
    }
    let exec = exec_words(&val("Exec"));
    if exec.is_empty() {
        return None;
    }
    let wm = val("StartupWMClass");
    Some(App {
        id: id.to_string(),
        name: val("Name"),
        comment: val("Comment"),
        exec,
        icon: val("Icon"),
        wm_class: (!wm.is_empty()).then_some(wm),
        terminal: val("Terminal") == "true",
        path: path.to_path_buf(),
    })
}

/// `Exec` as words: quotes as the desktop entry specification has them,
/// field codes removed (`%U`), `%%` is a percent sign.
pub fn exec_words(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let mut quoted = false;
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            '%' => {
                if it.next() == Some('%') {
                    cur.push('%');
                    any = true;
                }
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|w| !w.is_empty());
    out
}

/// The file of an icon by its desktop-file name: a PNG of about `size`
/// pixels in the hicolor theme or in pixmaps (scalable icons are not drawn).
pub fn icon_file(icon: &str, size: u32) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    let p = Path::new(icon);
    if p.is_absolute() {
        return (p.extension().is_some_and(|x| x == "png") && p.is_file()).then(|| p.to_path_buf());
    }
    let mut sizes = vec![size, 24, 32, 22, 48, 16, 64, 128];
    sizes.dedup();
    let mut roots = vec![paths::home().join(".icons")];
    roots.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    for s in &sizes {
        for r in &roots {
            let f = r.join(format!("hicolor/{s}x{s}/apps/{icon}.png"));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    let f = PathBuf::from(format!("/usr/share/pixmaps/{icon}.png"));
    f.is_file().then_some(f)
}

/// An icon as `size` x `size` ARGB pixels (premultiplied by nothing: the
/// alpha is in the top byte), or none when there is no PNG for it.
pub fn icon_pixels(icon: &str, size: u32) -> Option<Vec<u32>> {
    let path = icon_file(icon, size)?;
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let info = reader.info();
    if info.width > 512 || info.height > 512 {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let ch = match frame.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return None,
    };
    let px = |x: usize, y: usize| -> [u32; 4] {
        let i = (y * w + x) * ch;
        let b = &buf[i..i + ch];
        match ch {
            4 => [b[0] as u32, b[1] as u32, b[2] as u32, b[3] as u32],
            3 => [b[0] as u32, b[1] as u32, b[2] as u32, 255],
            2 => [b[0] as u32, b[0] as u32, b[0] as u32, b[1] as u32],
            _ => [b[0] as u32, b[0] as u32, b[0] as u32, 255],
        }
    };
    // A box filter down (or nearest up) to the size asked for.
    let s = size as usize;
    let mut out = Vec::with_capacity(s * s);
    for oy in 0..s {
        for ox in 0..s {
            let (x0, x1) = (ox * w / s, ((ox + 1) * w / s).max(ox * w / s + 1));
            let (y0, y1) = (oy * h / s, ((oy + 1) * h / s).max(oy * h / s + 1));
            let mut acc = [0u32; 4];
            let mut n = 0;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let p = px(x, y);
                    // Weight the colour by its alpha so edges stay clean.
                    for k in 0..3 {
                        acc[k] += p[k] * p[3];
                    }
                    acc[3] += p[3];
                    n += 1;
                }
            }
            let a = acc[3] / n.max(1);
            let c = |k: usize| acc[k].checked_div(acc[3]).unwrap_or(0);
            out.push((a << 24) | (c(0) << 16) | (c(1) << 8) | c(2));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classes_as_niruwm_reads_them() {
        assert_eq!(
            command_class(&v(&["kitty", "--class", "scratch-btop", "-e", "btop"])).as_deref(),
            Some("scratch-btop")
        );
        assert_eq!(
            command_class(&v(&["kitty", "--class=x"])).as_deref(),
            Some("x")
        );
        assert_eq!(command_class(&v(&["btop"])), None);
        assert_eq!(
            command_class(&v(&["sh", "-c", "exec nirunote", "--class", "nirunote"])).as_deref(),
            Some("nirunote")
        );
    }

    #[test]
    fn commands_get_a_class() {
        // A program with a class option gets it.
        assert_eq!(
            with_class(&v(&["niruterm", "-e", "btop"]), "scratch-btop"),
            v(&["niruterm", "--class", "scratch-btop", "-e", "btop"])
        );
        // Any other program runs through sh, which ignores the last words.
        assert_eq!(
            with_class(&v(&["nirufm", "/home/x y"]), "nirufm"),
            v(&["sh", "-c", "exec nirufm '/home/x y'", "--class", "nirufm"])
        );
        // A class already there is replaced, nothing is added.
        assert_eq!(
            with_class(
                &v(&["sh", "-c", "exec nirunote", "--class", "a"]),
                "nirunote"
            ),
            v(&["sh", "-c", "exec nirunote", "--class", "nirunote"])
        );
        assert_eq!(
            shown_command(&v(&["sh", "-c", "exec nirunote", "--class", "nirunote"])),
            "nirunote"
        );
        assert_eq!(
            program_of(&v(&["sh", "-c", "exec /usr/bin/nirunote", "--class", "x"])).as_deref(),
            Some("nirunote")
        );
    }

    #[test]
    fn desktop_files_and_scratchpads() {
        let t = "[Desktop Entry]\nType=Application\nName=NIRUFM\nExec=nirufm %U\nIcon=nirufm\nStartupWMClass=nirufm\n\n[Desktop Action new]\nName=New\nExec=nirufm --new\n";
        let a = parse("nirufm", Path::new("/x/nirufm.desktop"), t).unwrap();
        assert_eq!(a.exec, v(&["nirufm"]));
        assert_eq!(a.wm_class.as_deref(), Some("nirufm"));
        let (cmd, class) = scratchpad_command(&a, &v(&["niruterm"]));
        assert_eq!(cmd, v(&["sh", "-c", "exec nirufm", "--class", "nirufm"]));
        assert_eq!(class.unwrap().1, ClassFrom::Desktop);
        let btop = "[Desktop Entry]\nType=Application\nName=btop++\nExec=btop\nTerminal=true\n";
        let b = parse("btop", Path::new("/x/btop.desktop"), btop).unwrap();
        let (cmd, class) = scratchpad_command(&b, &v(&["niruterm"]));
        assert_eq!(
            cmd,
            v(&["niruterm", "--class", "scratch-btop", "-e", "btop"])
        );
        assert_eq!(class.unwrap(), ("scratch-btop".into(), ClassFrom::Command));
        let gimp = "[Desktop Entry]\nType=Application\nName=GIMP\nExec=gimp-2.10 %U\n";
        let g = parse("gimp", Path::new("/x/gimp.desktop"), gimp).unwrap();
        assert_eq!(class_for(&g.exec, Some(&g)).unwrap().1, ClassFrom::Guess);
        assert!(parse(
            "x",
            Path::new("/x"),
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n"
        )
        .is_none());
        assert_eq!(
            exec_words(r#"env "A=b c" prog --x=%f 100%%"#),
            v(&["env", "A=b c", "prog", "--x=", "100%"])
        );
    }
}
