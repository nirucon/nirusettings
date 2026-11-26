//! Checking a value against its schema entry, and turning what was typed
//! into a TOML value. A value that fails here is never written: the
//! component would refuse it (often the whole file with it).
use crate::schema::{plain, Kind, Setting, Split, Table};
use toml::Value;

/// Check `v` for setting `st`. `dynamic` holds run-time choices (themes)
/// for settings with `choices_from`; empty means "not known, skip".
pub fn value(st: &Setting, v: &Value, dynamic: &[String]) -> Result<(), String> {
    match st.kind {
        Kind::Bool => {
            v.as_bool().ok_or("must be true or false")?;
        }
        Kind::Int => {
            let n = v.as_integer().ok_or("must be a whole number")?;
            range(st, n as f64)?;
        }
        Kind::Float => {
            let n = v
                .as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .ok_or("must be a number")?;
            if !n.is_finite() {
                return Err("must be a number".into());
            }
            range(st, n)?;
        }
        Kind::Enum => {
            if !st.choices.is_empty() {
                if !st.choices.iter().any(|c| &c.value == v) {
                    return Err(format!("must be one of {}", st.choice_names()));
                }
            } else if let Some(s) = v.as_str() {
                if !dynamic.is_empty() && !dynamic.iter().any(|d| d == s) {
                    return Err(format!("{s} is not installed"));
                }
                if let Some(p) = &st.pattern {
                    pattern(p, s)?;
                }
            } else {
                return Err("must be a name".into());
            }
        }
        Kind::String | Kind::Path | Kind::Colour => {
            let s = v.as_str().ok_or("must be text")?;
            if s.contains('\n') || s.contains('\0') {
                return Err("must be one line".into());
            }
            if st.kind == Kind::Colour {
                pattern("hex", s)?;
            }
            if let Some(p) = &st.pattern {
                pattern(p, s)?;
            }
        }
        Kind::List => {
            let a = v.as_array().ok_or("must be a list")?;
            if let Some(min) = st.min_items {
                if a.len() < min {
                    return Err(if min == 1 {
                        "needs at least one entry".into()
                    } else {
                        format!("needs at least {min} entries")
                    });
                }
            }
            if let Some(max) = st.max_items {
                if a.len() > max {
                    return Err(format!("holds at most {max} entries"));
                }
            }
            for (i, item) in a.iter().enumerate() {
                let s = item.as_str().ok_or("entries must be text")?;
                if st.item_kind() == Kind::Enum && !st.choices.iter().any(|c| &c.value == item) {
                    return Err(format!(
                        "{s:?} is not one of {}",
                        st.choices
                            .iter()
                            .map(|c| plain(&c.value))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if s.contains('\n') || s.contains('\0') {
                    return Err("entries must be one line".into());
                }
                if let Some(p) = &st.pattern {
                    pattern(p, s)?;
                }
                if st.unique && a[..i].contains(item) {
                    return Err(format!("{s} is listed twice"));
                }
            }
        }
    }
    Ok(())
}

fn range(st: &Setting, n: f64) -> Result<(), String> {
    let lo = st.min.unwrap_or(f64::NEG_INFINITY);
    let hi = st.max.unwrap_or(f64::INFINITY);
    if n < lo || n > hi {
        let f = |x: f64| {
            crate::schema::fmt_float(x)
                .trim_end_matches(".0")
                .to_string()
        };
        return Err(match (st.min, st.max) {
            (Some(a), Some(b)) => format!("must be {} to {}", f(a), f(b)),
            (Some(a), None) => format!("must be at least {}", f(a)),
            (None, Some(b)) => format!("must be at most {}", f(b)),
            _ => unreachable!(),
        });
    }
    Ok(())
}

/// The named string checks.
pub fn pattern(name: &str, s: &str) -> Result<(), String> {
    let ok = match name {
        "nonempty" => !s.trim().is_empty(),
        "hhmm" => {
            let mut it = s.split(':');
            match (it.next(), it.next(), it.next()) {
                (Some(h), Some(m), None) if h.len() <= 2 && m.len() == 2 => {
                    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
                }
                _ => false,
            }
        }
        "hex" => {
            let t = s.trim().trim_start_matches('#');
            t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit())
        }
        "btaddr" => {
            s.trim().is_empty() || {
                let parts: Vec<&str> = s.trim().split(':').collect();
                parts.len() == 6
                    && parts
                        .iter()
                        .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
            }
        }
        // NIRUWM's rule for theme names.
        "theme" => {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        "word" => !s.is_empty() && !s.chars().any(char::is_whitespace),
        "strftime" => strftime(s),
        "keys" => return crate::keys::parse_keys(s).map(|_| ()),
        "action" => return crate::keys::check_action(s),
        _ => return Err(format!("unknown check {name:?} in the schema")),
    };
    if ok {
        Ok(())
    } else {
        Err(match name {
            "nonempty" => "must not be empty".into(),
            "hhmm" => "must be a time like 07:30".into(),
            "hex" => "must be a colour like 88c0d0".into(),
            "btaddr" => "must be a Bluetooth address like AA:BB:CC:DD:EE:FF, or empty".into(),
            "theme" => "letters, digits, - and _ only".into(),
            "word" => "one word, no spaces".into(),
            _ => "must be a date or time format like %Y-%m-%d".into(),
        })
    }
}

/// A conservative check of a chrono/strftime format: every `%` is followed
/// by an optional flag, width and colon, then a known conversion. Formats
/// the bar can only parse, never print (`%#z`), are refused.
fn strftime(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if i < b.len() && b"-_0^".contains(&b[i]) {
            i += 1;
        }
        if i < b.len() && b[i] == b'#' {
            return false;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        while i < b.len() && b[i] == b':' {
            i += 1;
        }
        if i >= b.len() || !b"aAbBcCdDeFfgGhHIjklmMnpPrRsStTuUVwWxXyYzZ%+".contains(&b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// Turn typed text into a value for `st`, then check it. Empty text on an
/// optional setting is `None`: remove the key.
pub fn parse_input(st: &Setting, text: &str, dynamic: &[String]) -> Result<Option<Value>, String> {
    let t = text.trim();
    if t.is_empty() && st.optional {
        return Ok(None);
    }
    let v = match st.kind {
        Kind::Bool => match t.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" | "1" => Value::Boolean(true),
            "false" | "off" | "no" | "0" => Value::Boolean(false),
            _ => return Err("type on or off".into()),
        },
        Kind::Int => Value::Integer(t.parse::<i64>().map_err(|_| "must be a whole number")?),
        Kind::Float => {
            let f = t
                .replace(',', ".")
                .parse::<f64>()
                .map_err(|_| "must be a number")?;
            Value::Float(f)
        }
        Kind::Enum => {
            // A choice by its value or its label, in any case.
            let lower = t.to_ascii_lowercase();
            if let Some(c) = st.choices.iter().find(|c| {
                plain(&c.value).to_ascii_lowercase() == lower
                    || c.label.to_ascii_lowercase() == lower
            }) {
                c.value.clone()
            } else {
                Value::String(t.to_string())
            }
        }
        Kind::String | Kind::Path => Value::String(text.trim_end_matches(['\n', '\r']).to_string()),
        Kind::Colour => Value::String(t.trim_start_matches('#').to_ascii_lowercase()),
        Kind::List => {
            let items = match st.split {
                Split::Comma => t
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
                Split::Words => words(t)?,
            };
            Value::Array(items.into_iter().map(Value::String).collect())
        }
    };
    value(st, &v, dynamic)?;
    Ok(Some(v))
}

/// Split a command line into words: spaces separate, "double quotes" keep
/// spaces, as do '...' (literal), \" is a quote. No other shell syntax.
pub fn words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // The open quote, if any: inside '...' everything is literal, inside
    // "..." a backslash still escapes, as in a shell.
    let mut quote: Option<char> = None;
    let mut any = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, quote) {
            ('\'', Some('\'')) | ('"', Some('"')) => quote = None,
            (c, Some('\'')) => cur.push(c),
            ('\\', _) => match chars.next() {
                Some(n) => {
                    cur.push(n);
                    any = true;
                }
                None => cur.push('\\'),
            },
            ('"' | '\'', None) => {
                quote = Some(c);
                any = true;
            }
            (c, None) if c.is_whitespace() => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (c, _) => {
                cur.push(c);
                any = true;
            }
        }
    }
    if quote.is_some() {
        return Err("a quote is not closed".into());
    }
    if any {
        out.push(cur);
    }
    Ok(out)
}

/// A list of words as it is typed (the reverse of [`words`]).
pub fn join_words(items: &[String]) -> String {
    items
        .iter()
        .map(|w| {
            if w.is_empty()
                || w.chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'')
            {
                format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                w.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A value as it is typed into the text field.
pub fn as_input(st: &Setting, v: Option<&Value>) -> String {
    let Some(v) = v else {
        return String::new();
    };
    match (st.kind, v) {
        (Kind::List, Value::Array(a)) => {
            let items: Vec<String> = a.iter().map(plain).collect();
            match st.split {
                Split::Words => join_words(&items),
                Split::Comma => items.join(", "),
            }
        }
        _ => plain(v),
    }
}

/// Check one item of a list of tables, in the numbers a person reads
/// (see [`crate::schema::Field::shown`]). Keys the schema does not know are
/// left alone: they came from the file.
pub fn item(t: &Table, item: &toml::Table) -> Result<(), String> {
    for f in &t.fields {
        let st = &f.setting;
        match item.get(&st.key) {
            Some(v) => value(st, v, &[]).map_err(|e| format!("{}: {e}", st.label))?,
            None if f.required => return Err(format!("{} is needed", st.label)),
            None => {}
        }
    }
    if t.editor.as_deref() == Some("scratchpad") {
        if let Some(cmd) = item.get("command").and_then(|v| v.as_array()) {
            let argv: Vec<String> = cmd
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect();
            if !argv.is_empty() && crate::desktop::command_class(&argv).is_none() {
                return Err("the command needs a window class (--class NAME), or NIRUWM cannot find its window".into());
            }
        }
    }
    Ok(())
}

/// Check a whole list of tables as it would be written (stored numbers).
pub fn table(t: &Table, items: &[toml::Table]) -> Result<(), String> {
    if let Some(max) = t.max_items {
        if items.len() > max {
            return Err(format!("{} holds at most {max} entries", t.label));
        }
    }
    let mut ids: Vec<String> = Vec::new();
    let mut classes: Vec<String> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let shown: toml::Table = it
            .iter()
            .map(|(k, v)| {
                let v = t.field(k).map(|f| f.shown(v)).unwrap_or_else(|| v.clone());
                (k.clone(), v)
            })
            .collect();
        let name = t
            .id_of(&shown)
            .map(|id| format!("{} {id}", t.item))
            .unwrap_or_else(|| format!("{} {}", t.item, i + 1));
        item(t, &shown).map_err(|e| format!("{name}: {e}"))?;
        if let Some(id) = t.id_of(&shown) {
            let key = id.to_lowercase();
            if ids.contains(&key) {
                return Err(format!("{name} is listed twice"));
            }
            ids.push(key);
            if let (Some([lo, hi]), Ok(n)) = (t.slots, id.parse::<i64>()) {
                if n < lo || n > hi {
                    return Err(format!("{name}: must be {lo} to {hi}"));
                }
            }
        }
        if t.editor.as_deref() == Some("scratchpad") {
            let argv: Vec<String> = it
                .get("command")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(c) = crate::desktop::command_class(&argv) {
                let c = c.to_lowercase();
                if classes.contains(&c) {
                    return Err(format!(
                        "{name}: the window class {c} is already another scratchpad's; each needs its own"
                    ));
                }
                classes.push(c);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_take_single_and_double_quotes() {
        let want = vec!["sh", "-c", "exec nirusys", "--class", "nirusys"];
        assert_eq!(words("sh -c 'exec nirusys' --class nirusys").unwrap(), want);
        assert_eq!(
            words("sh -c \"exec nirusys\" --class nirusys").unwrap(),
            want
        );
        assert_eq!(
            words("echo 'a \\ \"b\"'").unwrap(),
            vec!["echo", "a \\ \"b\""]
        );
        assert_eq!(words("it\\'s").unwrap(), vec!["it's"]);
        assert!(words("sh -c 'open").is_err());
        let back = join_words(&["a b".into(), "it's".into()]);
        assert_eq!(words(&back).unwrap(), vec!["a b", "it's"]);
    }

    fn setting(text: &str) -> Setting {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn numbers_ranges_and_enums() {
        let s = setting(
            "key='gap'\ntype='int'\nlabel='Gap'\nsection='Window manager'\ndefault=8\nmin=0\nmax=100",
        );
        assert!(value(&s, &Value::Integer(8), &[]).is_ok());
        assert_eq!(
            value(&s, &Value::Integer(101), &[]).unwrap_err(),
            "must be 0 to 100"
        );
        assert!(value(&s, &Value::String("8".into()), &[]).is_err());
        assert_eq!(
            parse_input(&s, " 12 ", &[]).unwrap(),
            Some(Value::Integer(12))
        );
        let e = setting(
            "key='p'\ntype='enum'\nlabel='P'\nsection='Bar'\ndefault='top'\nchoices=['top','bottom']",
        );
        assert!(value(&e, &Value::String("left".into()), &[]).is_err());
        assert_eq!(
            parse_input(&e, "Bottom", &[]).unwrap(),
            Some(Value::String("bottom".into()))
        );
        let f = setting(
            "key='r'\ntype='float'\nlabel='R'\nsection='Window manager'\ndefault=0.55\nmin=0.1\nmax=0.9",
        );
        assert_eq!(
            parse_input(&f, "0,6", &[]).unwrap(),
            Some(Value::Float(0.6))
        );
        assert!(parse_input(&f, "1.5", &[]).is_err());
    }

    #[test]
    fn patterns() {
        assert!(pattern("hhmm", "07:00").is_ok());
        assert!(pattern("hhmm", "24:00").is_err());
        assert!(pattern("hex", "#88C0d0").is_ok());
        assert!(pattern("hex", "88c0d").is_err());
        assert!(pattern("btaddr", "").is_ok());
        assert!(pattern("btaddr", "aa:bb:cc:dd:ee:ff").is_ok());
        assert!(pattern("btaddr", "aa:bb").is_err());
        assert!(pattern("strftime", "%Y-%m-%d").is_ok());
        assert!(pattern("strftime", "%a %-d %b, %H:%M").is_ok());
        assert!(pattern("strftime", "%#z").is_err());
        assert!(pattern("strftime", "%Q").is_err());
        assert!(pattern("strftime", "").is_err());
        assert!(pattern("theme", "niru-nord").is_ok());
        assert!(pattern("theme", "../x").is_err());
    }

    #[test]
    fn lists_and_words() {
        assert_eq!(
            words(r#"kitty --class "my pad" -e btop"#).unwrap(),
            vec!["kitty", "--class", "my pad", "-e", "btop"]
        );
        assert!(words("\"open").is_err());
        let items: Vec<String> = vec!["a b".into(), "c".into(), "".into()];
        assert_eq!(words(&join_words(&items)).unwrap(), items);
        let m = setting(
            "key='m'\ntype='list'\nitem='enum'\nunique=true\nlabel='M'\nsection='Bar'\ndefault=['clock']\nchoices=['clock','media']\nmin_items=1",
        );
        assert!(parse_input(&m, "clock, media", &[]).is_ok());
        assert_eq!(
            parse_input(&m, "clock, clock", &[]).unwrap_err(),
            "clock is listed twice"
        );
        assert!(parse_input(&m, "clock, weather", &[]).is_err());
        assert_eq!(
            parse_input(&m, "", &[]).unwrap_err(),
            "needs at least one entry"
        );
    }
}
