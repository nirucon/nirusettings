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

/// `--list [SECTION]` and `--list --json`.
pub fn list(store: &Store, section: Option<&str>, json: bool) -> String {
    let mut out = String::new();
    let mut rows = Vec::new();
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

/// `--get ID`: the value as the component uses it.
pub fn get(store: &Store, name: &str) -> Result<String, String> {
    let e = find(store, name)?;
    let cur = store.current(e, &dynamic(store, e));
    Ok(cur.value.as_ref().map(schema::plain).unwrap_or_default())
}

/// `--set ID VALUE` or `--reset ID`: check, write and apply.
pub fn set(store: &mut Store, name: &str, value: Option<&str>) -> Result<String, String> {
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
