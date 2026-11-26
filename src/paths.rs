//! Where things are. Every path honours the XDG variables; the
//! configuration files can also be pointed elsewhere as a whole with
//! `NIRUSETTINGS_ROOT` (the tests do this, so they never touch real files).
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

/// The directory that holds `niruwm/`, `niruui/` and `nirux/`.
pub fn config_home() -> PathBuf {
    if let Some(r) = std::env::var_os("NIRUSETTINGS_ROOT").filter(|r| !r.is_empty()) {
        return PathBuf::from(r);
    }
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn data_home() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("nirusettings")
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("nirusettings")
}

/// `~/x` -> `$HOME/x`.
pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        return home();
    }
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(p),
    }
}

/// `$HOME/x` -> `~/x`, for display.
pub fn tilde(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(rest) if h != Path::new("/") => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

/// Write a file through a temporary name in the same directory, flushed to
/// disk before the rename, keeping the old file's permissions.
pub fn write_atomic(p: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = p.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.nirusettings.{}", std::process::id()));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        if let Ok(meta) = std::fs::metadata(p) {
            std::fs::set_permissions(&tmp, meta.permissions())?;
        }
        std::fs::rename(&tmp, p)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// The first directory of `$PATH` with an executable `name`.
pub fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    if name.contains('/') {
        let p = expand(name);
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_keeps_mode_and_leaves_no_temp() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("nirusettings-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.toml");
        std::fs::write(&f, "x = 1\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&f, b"x = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x = 2\n");
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
