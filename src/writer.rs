//! Writing a change: start from the file on disk, edit only the touched
//! keys, let the component check the result, keep a backup of the old
//! file once per session, write it in one step, then apply it.
use crate::paths;
use crate::sources;
use crate::store::{edit_text, Change};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One write: a file and the changes to it.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub file: usize,
    /// Relative to the config home.
    pub rel: String,
    pub path: PathBuf,
    /// The text of a new file (the schema's `new_file`).
    pub new_file: String,
    pub changes: Vec<Change>,
    /// Checker commands (run with XDG_CONFIG_HOME at the candidate tree).
    pub checks: Vec<Vec<String>>,
    pub links: Vec<String>,
    /// Commands run after the write.
    pub apply: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Done {
    pub id: u64,
    pub file: usize,
    /// The file's text now (written, or as it was when refused).
    pub text: Option<String>,
    /// Why nothing was written.
    pub error: Option<String>,
    /// The checkers that ran ("niruwm") and the ones not installed.
    pub checked: Vec<String>,
    pub unchecked: Vec<String>,
    /// Apply commands that failed, with their message.
    pub apply_errors: Vec<String>,
    pub backup: Option<PathBuf>,
}

/// Keeps the per-session state: which files were backed up already.
#[derive(Default)]
pub struct Writer {
    backed_up: HashSet<PathBuf>,
    serial: u64,
}

impl Writer {
    pub fn run(&mut self, job: &Job) -> Done {
        let mut done = Done {
            id: job.id,
            file: job.file,
            text: None,
            error: None,
            checked: Vec::new(),
            unchecked: Vec::new(),
            apply_errors: Vec::new(),
            backup: None,
        };
        let disk = match std::fs::read_to_string(&job.path) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                done.error = Some(format!("cannot read {}: {e}", paths::tilde(&job.path)));
                return done;
            }
        };
        let base = disk.clone().unwrap_or_else(|| job.new_file.clone());
        done.text = disk.clone();
        let candidate = match edit_text(&base, &job.changes) {
            Ok(t) => t,
            Err(e) => {
                done.error = Some(e);
                return done;
            }
        };
        if Some(&candidate) == disk.as_ref() {
            done.text = Some(candidate);
            return done;
        }
        for argv in &job.checks {
            let Some(name) = argv.first() else { continue };
            if paths::which(name).is_none() {
                if !done.unchecked.contains(name) {
                    done.unchecked.push(name.clone());
                }
                continue;
            }
            self.serial += 1;
            match check(argv, &job.rel, &candidate, &job.links, self.serial) {
                Ok(()) => done.checked.push(name.clone()),
                Err(e) => {
                    done.error = Some(format!("{name} refuses it: {e}"));
                    return done;
                }
            }
        }
        if let Some(old) = &disk {
            if self.backed_up.insert(job.path.clone()) {
                let bak = backup_path(&job.path);
                if let Err(e) = paths::write_atomic(&bak, old.as_bytes()) {
                    done.error = Some(format!("cannot keep a backup {}: {e}", paths::tilde(&bak)));
                    self.backed_up.remove(&job.path);
                    return done;
                }
                done.backup = Some(bak);
            }
        }
        if let Err(e) = paths::write_atomic(&job.path, candidate.as_bytes()) {
            done.error = Some(format!("cannot write {}: {e}", paths::tilde(&job.path)));
            return done;
        }
        done.text = Some(candidate);
        if std::env::var_os("NIRUSETTINGS_NO_APPLY").is_none() {
            for argv in &job.apply {
                let Some(name) = argv.first() else { continue };
                if paths::which(name).is_none() {
                    done.apply_errors.push(format!("{name} is not installed"));
                    continue;
                }
                match sources::run(argv, &[], Duration::from_secs(10)) {
                    Some(o) if o.ok => {}
                    Some(o) => done.apply_errors.push(format!(
                        "{}: {}",
                        argv.join(" "),
                        first_line(&o.stderr)
                    )),
                    None => done
                        .apply_errors
                        .push(format!("{} did not finish", argv.join(" "))),
                }
            }
        }
        done
    }
}

/// `config.toml` -> `config.toml.bak`.
pub fn backup_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

fn first_line(s: &str) -> String {
    let l = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("failed");
    l.chars().take(200).collect()
}

/// Run one checker on a candidate: a temporary config home holds the
/// candidate at its place, the real files next to it (linked, read only
/// for the checker) and the `links` the checker also reads.
pub fn check(
    argv: &[String],
    rel: &str,
    candidate: &str,
    links: &[String],
    serial: u64,
) -> Result<(), String> {
    let real = paths::config_home();
    let tmp = paths::cache_dir().join(format!("check-{}-{serial}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let res = (|| {
        let target = tmp.join(rel);
        let dir = target.parent().ok_or("bad path")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(&target, candidate).map_err(|e| e.to_string())?;
        // The file's neighbours: NIRUWM reads its themes and appearance.toml.
        let real_dir = real.join(rel).parent().map(Path::to_path_buf);
        if let Some(rd) = real_dir {
            for e in std::fs::read_dir(&rd).into_iter().flatten().flatten() {
                let name = e.file_name();
                let to = dir.join(&name);
                if !to.exists() && !name.to_string_lossy().ends_with(".bak") {
                    let _ = std::os::unix::fs::symlink(e.path(), to);
                }
            }
        }
        for l in links {
            let from = real.join(l);
            let to = tmp.join(l);
            if from.exists() && !to.exists() {
                if let Some(p) = to.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                let _ = std::os::unix::fs::symlink(from, to);
            }
        }
        let o = sources::run(
            argv,
            &[("XDG_CONFIG_HOME", tmp.as_os_str())],
            Duration::from_secs(10),
        )
        .ok_or("the check did not finish")?;
        if o.ok {
            Ok(())
        } else {
            let msg = if o.stderr.trim().is_empty() {
                o.stdout
            } else {
                o.stderr
            };
            Err(first_line(&msg))
        }
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res
}
