//! The command line: list, read and change settings without the window,
//! with the same checks (`nirusettings --set bar.position bottom`).
use crate::schema;
use crate::store::{Source, Store};
use crate::writer::{Job, Writer};

/// The id of a setting: `component:key` (`nirulock:layout`).
pub fn id(store: &Store, e: usize) -> String {
    format!(
        "{}:{}",
        store.schema_of(e).component,
        store.entries[e].setting.key
    )
}

/// Find a setting by `component:key`, or by a key only one component has.
pub fn find(store: &Store, name: &str) -> Result<usize, String> {
    if let Some(i) = (0..store.entries.len()).find(|&i| id(store, i) == name) {
        return Ok(i);
    }
    let hits: Vec<usize> = (0..store.entries.len())
        .filter(|&i| store.entries[i].setting.key == name)
        .collect();
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(format!("no setting {name} (see nirusettings --list)")),
        _ => Err(format!(
            "{name} is in several files: {}",
            hits.iter()
                .map(|&i| id(store, i))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn dynamic(store: &Store, e: usize) -> Vec<String> {
    store.entries[e]
        .setting
        .choices_from
        .as_deref()
        .map(crate::store::dynamic_choices)
        .unwrap_or_default()
}

/// The id of a list of tables: `niruwm:scratchpad.slot`.
pub fn table_id(store: &Store, t: usize) -> String {
    let te = &store.tables[t];
    format!("{}:{}", store.schemas[te.schema].component, te.table.key)
}

/// Find a list of tables by `component:key` or by its key.
pub fn find_table(store: &Store, name: &str) -> Option<usize> {
    (0..store.tables.len())
        .find(|&t| table_id(store, t) == name)
        .or_else(|| {
            let hits: Vec<usize> = (0..store.tables.len())
                .filter(|&t| store.tables[t].table.key == name)
                .collect();
            (hits.len() == 1).then(|| hits[0])
        })
}

/// One item as inline TOML, the way the file holds it, with the numbers
/// the window shows in a comment where they differ.
pub fn item_line(t: &schema::Table, item: &toml::Table) -> String {
    let fields: Vec<String> = item
        .iter()
        .map(|(k, v)| format!("{k} = {}", crate::store::to_edit(v)))
        .collect();
    let mut line = format!("{{ {} }}", fields.join(", "));
    let shown: Vec<String> = t
        .fields
        .iter()
        .filter(|f| f.offset != 0)
        .filter_map(|f| {
            let v = item.get(&f.setting.key)?;
            Some(format!(
                "{} {} on screen",
                f.setting.key,
                schema::plain(&f.shown(v))
            ))
        })
        .collect();
    if !shown.is_empty() {
        line.push_str(&format!("  # {}", shown.join(", ")));
    }
    line
}

/// A short summary of a list: `2 slots`.
pub fn table_summary(store: &Store, t: usize) -> String {
    let n = store.items(t).len();
    let noun = &store.tables[t].table.item;
    match n {
        0 => "none".into(),
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// `--list [SECTION]` and `--list --json`.
pub fn list(store: &Store, section: Option<&str>, json: bool) -> String {
    let mut out = String::new();
    let mut rows = Vec::new();
    for t in 0..store.tables.len() {
        let tb = &store.tables[t].table;
        if let Some(s) = section {
            if !tb.section.eq_ignore_ascii_case(s) {
                continue;
            }
        }
        let items = store.items(t);
        let source = if store.table_in_file(t) {
            "file"
        } else {
            "default"
        };
        if json {
            rows.push(serde_json::json!({
                "id": table_id(store, t),
                "section": tb.section,
                "group": tb.group,
                "label": tb.label,
                "type": "table",
                "value": items.iter().map(|i| serde_json::to_value(i).unwrap_or_default()).collect::<Vec<_>>(),
                "source": source,
                "modified": !items.is_empty(),
                "file": store.files[store.tables[t].file].rel,
            }));
        } else {
            let mark = if items.is_empty() { " " } else { "*" };
            out.push_str(&format!(
                "{mark} {:<44} {}  [{source}]\n",
                table_id(store, t),
                table_summary(store, t)
            ));
        }
    }
    for e in 0..store.entries.len() {
        let st = &store.entries[e].setting;
        if let Some(s) = section {
            if !st.section.eq_ignore_ascii_case(s) {
                continue;
            }
        }
        let cur = store.current(e, &dynamic(store, e));
        let value = cur.value.as_ref().map(schema::plain).unwrap_or_default();
        let source = match cur.source {
            Source::Default => "default",
            Source::File => "file",
            Source::Unset => "unset",
        };
        if json {
            rows.push(serde_json::json!({
                "id": id(store, e),
                "section": st.section,
                "group": st.group,
                "label": st.label,
                "type": st.kind.name(),
                "value": cur.value.as_ref().map(|v| serde_json::to_value(v).unwrap_or_default()),
                "source": source,
                "modified": store.modified(e, &cur),
                "file": store.file_of(e).rel,
            }));
        } else {
            let mark = if store.modified(e, &cur) { "*" } else { " " };
            out.push_str(&format!(
                "{mark} {:<44} {value}  [{source}]\n",
                id(store, e)
            ));
        }
    }
    if json {
        out = serde_json::Value::Array(rows).to_string();
        out.push('\n');
    }
    out
}

/// `--get ID`: the value as the component uses it. A list of tables is
/// one inline table per line.
pub fn get(store: &Store, name: &str) -> Result<String, String> {
    if let Some(t) = find_table(store, name) {
        let tb = &store.tables[t].table;
        return Ok(store
            .items(t)
            .iter()
            .map(|i| item_line(tb, i))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let e = find(store, name)?;
    let cur = store.current(e, &dynamic(store, e));
    Ok(cur.value.as_ref().map(schema::plain).unwrap_or_default())
}

/// Write list `t` as `items` (stored numbers): check, write, apply.
pub fn write_table(store: &mut Store, t: usize, items: Vec<toml::Table>) -> Result<String, String> {
    let tb = store.tables[t].table.clone();
    crate::validate::table(&tb, &items)?;
    let fi = store.tables[t].file;
    if let Some(err) = &store.files[fi].error {
        return Err(err.clone());
    }
    let change = store.table_change(t, &items);
    let mode = table_mode(store, t).clone();
    let job = job_for_file(store, fi, vec![change], std::slice::from_ref(&mode.run), 1);
    let done = Writer::default().run(&job);
    if let Some(err) = done.error {
        return Err(err);
    }
    let mut msg = format!("{} = {}", table_id(store, t), {
        store.reload(fi);
        table_summary(store, t)
    });
    if !done.unchecked.is_empty() {
        msg.push_str(&format!(
            " (not checked by {}: not installed)",
            done.unchecked.join(", ")
        ));
    }
    msg.push_str(&format!("; {}", mode.says));
    for w in done.warnings.iter().chain(&done.apply_errors) {
        msg.push_str(&format!("; {w}"));
    }
    Ok(msg)
}

/// The apply mode of list `t`.
pub fn table_mode(store: &Store, t: usize) -> &schema::Apply {
    let te = &store.tables[t];
    let s = &store.schemas[te.schema];
    let name = te.table.apply.as_ref().unwrap_or(&s.apply);
    &s.modes[name]
}

/// `--set ID VALUE` for a list: VALUE is a TOML list of inline tables as
/// the file holds them: `[{ number = 1, command = ["kitty", "--class", "x"] }]`.
fn set_table(store: &mut Store, t: usize, value: Option<&str>) -> Result<String, String> {
    let items = match value {
        None => Vec::new(),
        Some(text) => {
            let doc: toml::Table =
                format!("x = {text}")
                    .parse()
                    .map_err(|e: toml::de::Error| {
                        format!("not a TOML list of tables: {}", e.message())
                    })?;
            doc["x"]
                .as_array()
                .ok_or("not a list")?
                .iter()
                .map(|v| {
                    v.as_table()
                        .cloned()
                        .ok_or("every entry must be a table { ... }")
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    write_table(store, t, items)
}

/// `--add ID FIELD=VALUE...`: one more item, the fields typed as in the
/// window (rule workspaces from 1). An item with the same id (a slot
/// number) is replaced.
pub fn add(store: &mut Store, name: &str, args: &[String]) -> Result<String, String> {
    let t = find_table(store, name)
        .ok_or_else(|| format!("no list {name} (see nirusettings --list)"))?;
    let tb = store.tables[t].table.clone();
    let mut shown = toml::Table::new();
    for a in args {
        let (k, v) = a
            .split_once('=')
            .ok_or_else(|| format!("{a}: write FIELD=VALUE"))?;
        let f = tb.field(k.trim()).ok_or_else(|| {
            format!(
                "{k} is not a field of {name} (fields: {})",
                tb.fields
                    .iter()
                    .map(|f| f.setting.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if let Some(v) =
            crate::validate::parse_input(&f.setting, v, &[]).map_err(|e| format!("{k}: {e}"))?
        {
            shown.insert(k.trim().to_string(), v);
        }
    }
    crate::validate::item(&tb, &shown)?;
    // In the schema's field order, as a person would write them.
    let stored: toml::Table = tb
        .fields
        .iter()
        .filter_map(|f| {
            let v = shown.get(&f.setting.key)?;
            Some((f.setting.key.clone(), f.stored(v)))
        })
        .collect();
    let mut items = store.items(t);
    match tb
        .id_of(&stored)
        .and_then(|id| items.iter().position(|i| tb.id_of(i) == Some(id.clone())))
    {
        Some(i) => items[i] = stored,
        None => items.push(stored),
    }
    write_table(store, t, items)
}

/// `--remove ID WHICH`: WHICH is the item's id (a slot number, a key) or
/// its place in the list, from 1.
pub fn remove(store: &mut Store, name: &str, which: &str) -> Result<String, String> {
    let t = find_table(store, name)
        .ok_or_else(|| format!("no list {name} (see nirusettings --list)"))?;
    let tb = store.tables[t].table.clone();
    let mut items = store.items(t);
    let by_id = items
        .iter()
        .position(|i| tb.id_of(i).is_some_and(|id| id.eq_ignore_ascii_case(which)));
    // Else by the first field (a rule's class, a binding's keys), else by
    // its place.
    let first = &tb.fields[0].setting.key;
    let by_first: Vec<usize> = (0..items.len())
        .filter(|&i| {
            items[i]
                .get(first)
                .is_some_and(|v| schema::plain(v).eq_ignore_ascii_case(which))
        })
        .collect();
    let i = match (by_id, by_first.as_slice()) {
        (Some(i), _) => i,
        (None, [i]) => *i,
        (None, [_, _, ..]) => {
            return Err(format!(
                "{which} is in several entries; give the place in the list instead"
            ))
        }
        (None, []) => match which.parse::<usize>() {
            Ok(n) if n >= 1 && n <= items.len() && tb.id_field.is_none() => n - 1,
            _ => return Err(format!("{name} has no {} {which}", tb.item)),
        },
    };
    items.remove(i);
    write_table(store, t, items)
}

/// `--set ID VALUE` or `--reset ID`: check, write and apply.
pub fn set(store: &mut Store, name: &str, value: Option<&str>) -> Result<String, String> {
    if let Some(t) = find_table(store, name) {
        return set_table(store, t, value);
    }
    let e = find(store, name)?;
    let st = store.entries[e].setting.clone();
    if st.read_only {
        return Err(format!("{name} is shown, not changed, here"));
    }
    let dynamic = dynamic(store, e);
    let v = match value {
        Some(text) => crate::validate::parse_input(&st, text, &dynamic)?,
        None => st.default.clone(),
    };
    if let Some(v) = &v {
        let bin = &store.schema_of(e).binary;
        let installed = if bin.is_empty() {
            None
        } else {
            crate::sources::version(bin)
        };
        if let Some(why) = st.too_new(v, installed.as_deref()) {
            return Err(why);
        }
    }
    let fi = store.entries[e].file;
    if let Some(err) = &store.files[fi].error {
        return Err(err.clone());
    }
    let job = job_for(store, e, v, 1);
    let done = Writer::default().run(&job);
    if let Some(err) = done.error {
        return Err(err);
    }
    let mut msg = format!("{} = {}", id(store, e), get_text(&job));
    if !done.unchecked.is_empty() {
        msg.push_str(&format!(
            " (not checked by {}: not installed)",
            done.unchecked.join(", ")
        ));
    }
    let mode = apply_mode(store, e);
    msg.push_str(&format!("; {}", mode.says));
    for a in done.apply_errors {
        msg.push_str(&format!("; {a}"));
    }
    store.reload(fi);
    Ok(msg)
}

fn get_text(job: &Job) -> String {
    job.changes[0]
        .value
        .as_ref()
        .map(schema::plain)
        .unwrap_or_else(|| "(removed)".into())
}

/// The apply mode of entry `e`.
pub fn apply_mode(store: &Store, e: usize) -> &schema::Apply {
    let s = store.schema_of(e);
    let name = store.entries[e].setting.apply.as_ref().unwrap_or(&s.apply);
    &s.modes[name]
}

/// The write job for setting entry `e` to `value`.
pub fn job_for(store: &Store, e: usize, value: Option<toml::Value>, id: u64) -> Job {
    let changes = store.changes_for(e, value);
    let fi = store.entries[e].file;
    job_for_file(store, fi, changes, &[apply_mode(store, e).run.clone()], id)
}

/// A write job for file `fi` with every checker of every schema of it.
pub fn job_for_file(
    store: &Store,
    fi: usize,
    changes: Vec<crate::store::Change>,
    apply: &[Vec<String>],
    id: u64,
) -> Job {
    let f = &store.files[fi];
    let mut checks: Vec<Vec<String>> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    for &si in &f.schemas {
        for c in &store.schemas[si].check {
            if !checks.contains(c) {
                checks.push(c.clone());
            }
        }
        for l in &store.schemas[si].check_links {
            if !links.contains(l) {
                links.push(l.clone());
            }
        }
    }
    let mut runs: Vec<Vec<String>> = Vec::new();
    for a in apply {
        if !a.is_empty() && !runs.contains(a) {
            runs.push(a.clone());
        }
    }
    Job {
        id,
        file: fi,
        rel: f.rel.clone(),
        path: f.path.clone(),
        new_file: store.schemas[f.schemas[0]].new_file.clone(),
        changes,
        checks,
        links,
        apply: runs,
    }
}
