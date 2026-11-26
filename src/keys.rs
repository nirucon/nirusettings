//! Key bindings: NIRUWM's `[[keys]]` entries, how they combine with its
//! built-in table, and what changing one means.
//!
//! NIRUWM starts from its built-in bindings; every `[[keys]]` entry replaces
//! the built-in binding on the same keys (or adds one), and `action =
//! "unbind"` releases a key. So moving a built-in action to another key is
//! two entries: the action on the new key, `unbind` on the old one. The
//! rules here mirror niruwm `Config::effective_keys`, `bindings::parse_keys`
//! and `bindings::shadowed`; `niruwm --check-config` has the last word on
//! every write.
use std::collections::HashMap;

/// One binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub keys: String,
    pub action: String,
    pub description: Option<String>,
}

impl Binding {
    pub fn from_table(t: &toml::Table) -> Option<Binding> {
        Some(Binding {
            keys: t.get("keys")?.as_str()?.to_string(),
            action: t.get("action")?.as_str()?.to_string(),
            description: t
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string),
        })
    }

    pub fn to_table(&self) -> toml::Table {
        let mut t = toml::Table::new();
        t.insert("keys".into(), toml::Value::String(self.keys.clone()));
        t.insert("action".into(), toml::Value::String(self.action.clone()));
        if let Some(d) = self.description.as_ref().filter(|d| !d.trim().is_empty()) {
            t.insert("description".into(), toml::Value::String(d.clone()));
        }
        t
    }

    pub fn unbinds(&self) -> bool {
        self.action.trim() == "unbind"
    }
}

const SUPER: u32 = 1 << 6;
const SHIFT: u32 = 1;
const CTRL: u32 = 1 << 2;
const ALT: u32 = 1 << 3;

/// `super+shift+2` -> (modifier mask, X keysym name), as NIRUWM reads it.
pub fn parse_keys(spec: &str) -> Result<(u32, String), String> {
    let spec = spec.trim();
    let (mods, key) = match spec.rfind('+') {
        // `super++` is not a key NIRUWM knows; `plus` is not either.
        Some(i) => (&spec[..i], &spec[i + 1..]),
        None => ("", spec),
    };
    let mut mask = 0;
    for part in mods.split('+') {
        let t = part.trim();
        if t.is_empty() {
            continue;
        }
        mask |= match t.to_ascii_lowercase().as_str() {
            "super" | "mod4" => SUPER,
            "shift" => SHIFT,
            "ctrl" | "control" => CTRL,
            "alt" | "mod1" => ALT,
            "mod3" => 1 << 5,
            other => return Err(format!("unknown modifier {other}")),
        };
    }
    Ok((mask, expand_key(key)?))
}

fn expand_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("no key after the modifiers".into());
    }
    let named = match k.to_ascii_lowercase().as_str() {
        "enter" | "return" => "Return",
        "space" => "space",
        "tab" => "Tab",
        "escape" | "esc" => "Escape",
        "backtick" | "grave" => "grave",
        "print" | "printscreen" => "Print",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "comma" => "comma",
        "period" => "period",
        "slash" => "slash",
        "minus" => "minus",
        "equal" => "equal",
        _ => {
            if k.chars().count() == 1 {
                return Ok(k.to_string());
            }
            if k.starts_with('F') && k[1..].chars().all(|c| c.is_ascii_digit()) {
                return Ok(k.to_string());
            }
            if k.starts_with("XF86") {
                return Ok(k.to_string());
            }
            return Err(format!("unknown key {k}"));
        }
    };
    Ok(named.to_string())
}

/// Two key specs name the same combination (`Super+T` and `super+t`).
pub fn same_keys(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
        || matches!((parse_keys(a), parse_keys(b)), (Ok(x), Ok(y)) if x == y)
}

/// A key spec as NIRUWM writes it: `super+shift+t`.
pub fn normal(spec: &str) -> String {
    let Ok((mask, key)) = parse_keys(spec) else {
        return spec.trim().to_string();
    };
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [
        (SUPER, "super"),
        (CTRL, "ctrl"),
        (ALT, "alt"),
        (SHIFT, "shift"),
    ] {
        if mask & bit != 0 {
            parts.push(name.into());
        }
    }
    if mask & (1 << 5) != 0 {
        parts.push("mod3".into());
    }
    parts.push(key);
    parts.join("+")
}

/// `super+shift+Return` -> `Super+Shift+Return`.
pub fn label(spec: &str) -> String {
    crate::sources::key_label(spec)
}

/// The X keysym of a key name, as NIRUWM resolves it.
pub fn keysym_named(name: &str) -> Option<u32> {
    let v = match name {
        "Return" => 0xff0d,
        "Tab" => 0xff09,
        "Escape" => 0xff1b,
        "space" => 0x20,
        "grave" => 0x60,
        "minus" => 0x2d,
        "equal" => 0x3d,
        "comma" => 0x2c,
        "period" => 0x2e,
        "slash" => 0x2f,
        "semicolon" => 0x3b,
        "apostrophe" => 0x27,
        "bracketleft" => 0x5b,
        "bracketright" => 0x5d,
        "backslash" => 0x5c,
        "Left" => 0xff51,
        "Up" => 0xff52,
        "Right" => 0xff53,
        "Down" => 0xff54,
        "Print" => 0xff61,
        "BackSpace" => 0xff08,
        "Delete" => 0xffff,
        "XF86AudioMute" => 0x1008ff12,
        "XF86AudioLowerVolume" => 0x1008ff11,
        "XF86AudioRaiseVolume" => 0x1008ff13,
        "XF86AudioPlay" => 0x1008ff14,
        "XF86AudioStop" => 0x1008ff15,
        "XF86AudioPrev" => 0x1008ff16,
        "XF86AudioNext" => 0x1008ff17,
        "XF86AudioPause" => 0x1008ff31,
        "XF86AudioMicMute" => 0x1008ffb2,
        "XF86MonBrightnessUp" => 0x1008ff02,
        "XF86MonBrightnessDown" => 0x1008ff03,
        "XF86Display" => 0x1008ff59,
        _ => {
            if name.chars().count() == 1 {
                u32::from(name.chars().next()?)
            } else {
                let n: u32 = name.strip_prefix('F')?.parse().ok()?;
                if !(1..=35).contains(&n) {
                    return None;
                }
                0xffbe + (n - 1)
            }
        }
    };
    Some(v)
}

/// The key name for a keysym the window was given (the reverse of
/// [`keysym_named`], for a key combination pressed in the editor).
pub fn keysym_name(sym: u32) -> Option<String> {
    let named = [
        (0xff0d, "Return"),
        (0xff09, "Tab"),
        (0xff1b, "Escape"),
        (0x20, "space"),
        (0x60, "grave"),
        (0x2d, "minus"),
        (0x3d, "equal"),
        (0x2c, "comma"),
        (0x2e, "period"),
        (0x2f, "slash"),
        (0x3b, "semicolon"),
        (0x27, "apostrophe"),
        (0x5b, "bracketleft"),
        (0x5d, "bracketright"),
        (0x5c, "backslash"),
        (0xff51, "Left"),
        (0xff52, "Up"),
        (0xff53, "Right"),
        (0xff54, "Down"),
        (0xff61, "Print"),
        (0xff08, "BackSpace"),
        (0xffff, "Delete"),
    ];
    if let Some((_, n)) = named.iter().find(|(s, _)| *s == sym) {
        return Some(n.to_string());
    }
    if (0xffbe..0xffbe + 35).contains(&sym) {
        return Some(format!("F{}", sym - 0xffbe + 1));
    }
    let c = char::from_u32(sym).filter(|c| c.is_ascii_graphic())?;
    Some(c.to_ascii_lowercase().to_string())
}

/// NIRUWM's table: the built-in bindings, each user entry replacing the
/// built-in one on the same keys. `unbind` entries stay in the list (the
/// caller skips them); they release a key.
pub fn effective(defaults: &[Binding], user: &[Binding]) -> Vec<Binding> {
    let mut table: Vec<Binding> = defaults.to_vec();
    for u in user {
        table.retain(|b| !same_keys(&b.keys, &u.keys));
        table.push(u.clone());
    }
    table
}

/// The current keyboard layout: for each keysym, the key that types it
/// and at which shift level (0 without Shift or AltGr).
#[derive(Clone, Debug, Default)]
pub struct Layout {
    by_sym: HashMap<u32, (u8, usize)>,
}

impl Layout {
    /// From the X keyboard mapping: `per` keysyms per keycode, from
    /// `min_keycode`.
    pub fn new(min_keycode: u8, per: usize, syms: &[u32]) -> Layout {
        let mut by_sym: HashMap<u32, (u8, usize)> = HashMap::new();
        for (i, row) in syms.chunks(per.max(1)).enumerate() {
            let code = (min_keycode as usize + i).min(255) as u8;
            for (level, s) in row.iter().enumerate() {
                if *s == 0 {
                    continue;
                }
                match by_sym.get(s) {
                    Some(&(_, l)) if l <= level => {}
                    _ => {
                        by_sym.insert(*s, (code, level));
                    }
                }
            }
        }
        Layout { by_sym }
    }

    pub fn is_empty(&self) -> bool {
        self.by_sym.is_empty()
    }

    /// The physical key and level of a key spec, when the layout has it.
    pub fn key_of(&self, spec: &str) -> Option<(u8, u32, usize)> {
        let (mods, key) = parse_keys(spec).ok()?;
        let sym = keysym_named(&key)?;
        let (code, level) = self.by_sym.get(&sym).copied()?;
        Some((code, mods, level))
    }
}

/// Bindings that are the same physical key on this layout (`super+slash`
/// and `super+7` on a Swedish keyboard): NIRUWM grabs keys, so it keeps the
/// one whose keysym the key types unshifted (on a tie the earlier one).
/// Returns (dropped, kept) indexes into `table`.
pub fn shadowed(table: &[Binding], layout: &Layout) -> Vec<(usize, usize)> {
    let mut keep: Vec<(usize, (u8, u32, usize))> = Vec::new();
    let mut dropped = Vec::new();
    for (i, b) in table.iter().enumerate() {
        if b.unbinds() {
            continue;
        }
        let Some(k) = layout.key_of(&b.keys) else {
            continue;
        };
        match keep.iter().position(|(_, o)| o.0 == k.0 && o.1 == k.1) {
            None => keep.push((i, k)),
            Some(pos) => {
                let (other, ok) = keep[pos];
                if k.2 < ok.2 {
                    dropped.push((other, i));
                    keep[pos] = (i, k);
                } else {
                    dropped.push((i, other));
                }
            }
        }
    }
    dropped
}

/// The `[[keys]]` entries after binding `new`, in place of the binding on
/// `old` keys when one is edited. Another entry on the new keys is
/// replaced; a built-in binding left on the old keys is released.
pub fn set_binding(
    user: &[toml::Table],
    defaults: &[Binding],
    old: Option<&str>,
    new: &Binding,
) -> Vec<toml::Table> {
    let keys_of = |t: &toml::Table| {
        t.get("keys")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string()
    };
    let edited = old.and_then(|o| user.iter().position(|t| same_keys(&keys_of(t), o)));
    let mut out: Vec<toml::Table> = Vec::new();
    for (i, t) in user.iter().enumerate() {
        if Some(i) == edited {
            // Keep keys the schema does not know; replace the rest.
            let mut n = t.clone();
            n.remove("description");
            for (k, v) in new.to_table() {
                n.insert(k, v);
            }
            out.push(n);
        } else if !same_keys(&keys_of(t), &new.keys) {
            out.push(t.clone());
        }
    }
    if edited.is_none() {
        out.push(new.to_table());
    }
    if let Some(o) = old {
        let moved = !same_keys(o, &new.keys);
        let builtin = defaults.iter().any(|d| same_keys(&d.keys, o));
        let held = out.iter().any(|t| same_keys(&keys_of(t), o));
        if moved && builtin && !held {
            out.push(
                Binding {
                    keys: o.to_string(),
                    action: "unbind".into(),
                    description: None,
                }
                .to_table(),
            );
        }
    }
    out
}

/// The entries after removing the binding on `keys`: a built-in one is
/// released with `unbind`, an own one is deleted.
pub fn remove_binding(user: &[toml::Table], defaults: &[Binding], keys: &str) -> Vec<toml::Table> {
    let keys_of = |t: &toml::Table| {
        t.get("keys")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string()
    };
    let builtin = defaults.iter().any(|d| same_keys(&d.keys, keys));
    if !builtin {
        return user
            .iter()
            .filter(|t| !same_keys(&keys_of(t), keys))
            .cloned()
            .collect();
    }
    let unbind = Binding {
        keys: keys.to_string(),
        action: "unbind".into(),
        description: None,
    };
    set_binding(user, defaults, Some(keys), &unbind)
}

/// The entries without any on `keys`: the built-in binding (if any) is
/// back.
pub fn restore_default(user: &[toml::Table], keys: &str) -> Vec<toml::Table> {
    user.iter()
        .filter(|t| !same_keys(t.get("keys").and_then(|k| k.as_str()).unwrap_or(""), keys))
        .cloned()
        .collect()
}

/// The actions a binding can run: (name, argument, what it does). From
/// niruwm `bindings::parse_action` and `DO_ACTIONS`.
pub const ACTIONS: &[(&str, &str, &str)] = &[
    ("terminal", "", "Open a terminal"),
    ("terminal_alt", "", "Open the alternative terminal"),
    ("browser", "", "Open the web browser"),
    ("start", "PROGRAM", "Start a program (no shell)"),
    ("shell", "COMMAND", "Run a shell command"),
    ("launcher", "", "Application launcher"),
    ("launcher_unified", "", "Unified launcher"),
    ("launcher_actions", "", "NIRU Actions"),
    ("launcher_search", "", "Web search"),
    ("launcher_clipboard", "", "Clipboard history"),
    ("launcher_keys", "", "Keybinding cheat sheet"),
    ("launcher_emoji", "", "Emoji and symbol picker"),
    ("launcher_files", "", "File search"),
    ("launcher_panel", "", "NIRUPANEL"),
    ("launcher_manual", "", "The NIRUX manual"),
    ("window_hints", "", "Jump to a visible window by its label"),
    ("window_switcher", "", "Window switcher"),
    ("overview", "", "Overview of every workspace"),
    ("quick_note", "", "Quick note to the inbox"),
    ("lock", "", "Lock the screen"),
    ("suspend", "", "Lock, then suspend the computer"),
    ("reload", "", "Reload the configuration"),
    ("quit", "", "End the session"),
    ("save_session", "", "Save the open windows as the session"),
    ("clear_session", "", "Forget the saved session"),
    ("theme", "NAME", "Switch to a theme"),
    ("tag", "N", "Go to workspace N"),
    ("toggletag", "N", "Show workspace N too"),
    ("movetotag", "N", "Move the window to workspace N"),
    (
        "togglemovetotag",
        "N",
        "Move the window to workspace N and follow it",
    ),
    ("tag_next", "", "Next workspace"),
    ("tag_prev", "", "Previous workspace"),
    ("focusmon", "1|-1", "Focus the next or previous screen"),
    (
        "tagmon",
        "1|-1",
        "Move the window to the next or previous screen",
    ),
    ("focus_next", "", "Focus the next window"),
    ("focus_prev", "", "Focus the previous window"),
    (
        "focus_urgent",
        "",
        "Go to the window that asked for attention",
    ),
    ("swap_next", "", "Move the window down the stack"),
    ("swap_prev", "", "Move the window up the stack"),
    ("rotate_next", "", "Rotate the stack down"),
    ("rotate_prev", "", "Rotate the stack up"),
    ("close", "", "Close the window"),
    ("toggle_fullscreen", "", "Fullscreen on or off"),
    ("toggle_float", "", "Floating on or off"),
    ("toggle_sticky", "", "Sticky on or off"),
    ("pip_toggle", "", "Picture in picture on or off"),
    ("pip_corner", "", "Picture in picture to the next corner"),
    (
        "layout",
        "tile|left|monocle|centered|centeredfloat|floating|fibonacci|scroll",
        "Set the layout",
    ),
    ("layout_next", "", "Next layout"),
    ("layout_prev", "", "Previous layout"),
    ("scroll_columns", "1|2|toggle", "Scroll layout columns"),
    ("inc_master", "", "One more window in the master area"),
    ("dec_master", "", "One window fewer in the master area"),
    ("inc_master_ratio", "", "Wider master area"),
    ("dec_master_ratio", "", "Narrower master area"),
    ("toggle_gaps", "", "Gaps on or off"),
    ("bar_top", "", "Bar at the top"),
    ("bar_bottom", "", "Bar at the bottom"),
    ("bar_hide", "", "Hide the bar"),
    ("bar_show", "", "Show the bar"),
    ("bar_toggle", "", "Show or hide the bar"),
    ("bar_cycle", "", "Bar: top, bottom, hidden"),
    (
        "scratchpad",
        "[N]",
        "Show or hide the default scratchpad, or slot N",
    ),
    (
        "scratchpad_send",
        "N",
        "Send the window to scratchpad N, or back",
    ),
    ("unscratch", "", "Make the scratchpad a normal window"),
    (
        "move_float",
        "left|right|up|down|DX DY",
        "Move a floating window",
    ),
    ("resize_float", "DX DY", "Resize a floating window"),
    ("media_key", "KEYSYM", "A media key, such as XF86AudioMute"),
    (
        "brightness_step",
        "N",
        "Screen brightness up or down by N %",
    ),
    ("nightlight_toggle", "", "Night light on or off"),
    ("dnd_toggle", "", "Do Not Disturb on or off"),
    ("idle_toggle", "", "Keep the screen awake on or off"),
    ("swallow_toggle", "", "Window swallowing on or off"),
    ("studio_toggle", "", "Studio mode on or off"),
    ("unbind", "", "Release the key: nothing happens"),
];

/// Aliases NIRUWM accepts for some actions.
const ALIASES: &[(&str, &str)] = &[
    ("move_next", "swap_next"),
    ("move_prev", "swap_prev"),
    ("rotatestack_next", "rotate_next"),
    ("rotatestack_prev", "rotate_prev"),
];

/// Check an action the way NIRUWM parses it.
pub fn check_action(spec: &str) -> Result<(), String> {
    let spec = spec.trim();
    let (verb, rest) = match spec.split_once(char::is_whitespace) {
        Some((v, r)) => (v, r.trim()),
        None => (spec, ""),
    };
    let verb = ALIASES
        .iter()
        .find(|(a, _)| *a == verb)
        .map(|(_, v)| *v)
        .unwrap_or(verb);
    let Some((_, arg, _)) = ACTIONS.iter().find(|(v, _, _)| *v == verb) else {
        return Err(if verb.is_empty() {
            "an action is needed".into()
        } else {
            format!("{verb} is not an action NIRUWM knows")
        });
    };
    let need = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(format!("{verb} needs {what}"))
        }
    };
    match *arg {
        "" if verb != "scratchpad" => need(rest.is_empty() || verb == "unbind", "no argument"),
        "N" if verb == "brightness_step" => need(rest.parse::<i16>().is_ok(), "a number"),
        "N" if verb == "scratchpad_send" => need(
            matches!(rest.parse::<u8>(), Ok(1..=9)),
            "a slot from 1 to 9",
        ),
        "N" => need(rest.parse::<u32>().is_ok(), "a number"),
        "[N]" => need(
            rest.is_empty() || matches!(rest.parse::<u8>(), Ok(1..=9)),
            "a slot from 1 to 9, or nothing",
        ),
        a if a.contains('|') && !a.contains(' ') => need(
            a.split('|').any(|x| x == rest),
            &format!("one of {}", a.replace('|', ", ")),
        ),
        "DX DY" => {
            let n: Vec<_> = rest.split_whitespace().map(|x| x.parse::<i16>()).collect();
            need(n.len() == 2 && n.iter().all(|x| x.is_ok()), "two numbers")
        }
        "left|right|up|down|DX DY" => {
            let n: Vec<_> = rest.split_whitespace().map(|x| x.parse::<i16>()).collect();
            need(
                ["left", "right", "up", "down"].contains(&rest)
                    || (n.len() == 2 && n.iter().all(|x| x.is_ok())),
                "left, right, up, down or two numbers",
            )
        }
        _ => need(!rest.is_empty(), "an argument"),
    }
}

/// What an action does, in words: `layout tile` -> `Set the layout: tile`.
pub fn describe(action: &str) -> String {
    let a = action.trim();
    let (verb, rest) = match a.split_once(char::is_whitespace) {
        Some((v, r)) => (v, r.trim()),
        None => (a, ""),
    };
    match ACTIONS.iter().find(|(v, _, _)| *v == verb) {
        Some((_, _, d)) if rest.is_empty() => d.to_string(),
        Some((_, _, d)) => {
            let words: Vec<&str> = d.split(' ').collect();
            if words.contains(&"N") {
                words
                    .iter()
                    .map(|w| if *w == "N" { rest } else { w })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                format!("{d}: {rest}")
            }
        }
        None => a.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(k: &str, a: &str) -> Binding {
        Binding {
            keys: k.into(),
            action: a.into(),
            description: None,
        }
    }

    #[test]
    fn keys_compare_and_merge_like_niruwm() {
        assert!(same_keys("Super+T", "super+t"));
        assert!(same_keys("super+shift+Return", "shift+super+enter"));
        assert!(!same_keys("super+t", "super+shift+t"));
        assert!(parse_keys("hyper+t").is_err());
        assert!(parse_keys("super+").is_err());
        assert_eq!(normal("Shift+Super+Enter"), "super+shift+Return");
        let d = vec![b("super+t", "layout tile"), b("super+p", "launcher")];
        let eff = effective(&d, &[b("super+t", "layout monocle")]);
        assert_eq!(
            eff,
            vec![b("super+p", "launcher"), b("super+t", "layout monocle")]
        );
    }

    #[test]
    fn the_same_physical_key_is_found() {
        // Swedish: keycode 16 types 7 and, with Shift, slash.
        let l = Layout::new(16, 2, &[0x37, 0x2f, 0x2d, 0x5f]);
        let t = vec![
            b("super+slash", "launcher_keys"),
            b("super+7", "tag 7"),
            b("super+minus", "x"),
        ];
        // super+7 is the key unshifted, so it wins over super+slash.
        assert_eq!(shadowed(&t, &l), vec![(0, 1)]);
        let us = Layout::new(16, 2, &[0x37, 0x26, 0x2f, 0x3f]);
        assert!(shadowed(&t, &us).is_empty());
    }

    #[test]
    fn editing_writes_the_fewest_entries() {
        let d = vec![b("super+p", "launcher"), b("super+t", "layout tile")];
        // Moving a built-in action releases the old key.
        let out = set_binding(&[], &d, Some("super+p"), &b("super+x", "launcher"));
        assert_eq!(
            out,
            vec![
                b("super+x", "launcher").to_table(),
                b("super+p", "unbind").to_table()
            ]
        );
        // Changing an own entry edits it in place, keeping unknown keys.
        let mut own = b("super+z", "start nirufm").to_table();
        own.insert("note".into(), toml::Value::String("mine".into()));
        let out = set_binding(
            &[own.clone()],
            &d,
            Some("super+z"),
            &b("super+y", "start nirufm"),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["keys"].as_str(), Some("super+y"));
        assert_eq!(out[0]["note"].as_str(), Some("mine"));
        // A new binding on a taken key replaces the entry there.
        let out = set_binding(&[own.clone()], &d, None, &b("Super+Z", "close"));
        assert_eq!(out, vec![b("Super+Z", "close").to_table()]);
        // Removing: a built-in key is released, an own entry goes.
        assert_eq!(
            remove_binding(&[], &d, "super+t"),
            vec![b("super+t", "unbind").to_table()]
        );
        assert!(remove_binding(&[own.clone()], &d, "super+z").is_empty());
        assert!(restore_default(&[b("super+t", "unbind").to_table()], "super+t").is_empty());
    }

    #[test]
    fn actions_are_checked() {
        assert!(check_action("layout tile").is_ok());
        assert!(check_action("layout tiles").is_err());
        assert!(check_action("start nirufm").is_ok());
        assert!(check_action("start").is_err());
        assert!(check_action("tag 3").is_ok());
        assert!(check_action("scratchpad").is_ok());
        assert!(check_action("scratchpad 10").is_err());
        assert!(check_action("move_float 10 -5").is_ok());
        assert!(check_action("fly").is_err());
        assert!(check_action("close now").is_err());
        assert_eq!(describe("tag 3"), "Go to workspace 3");
        assert_eq!(
            describe("start nirufm"),
            "Start a program (no shell): nirufm"
        );
        assert_eq!(keysym_name(0x2f).as_deref(), Some("slash"));
        assert_eq!(keysym_name(0x54).as_deref(), Some("t"));
    }
}
