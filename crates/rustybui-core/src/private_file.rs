//! Writes for files that reveal where the user is: saved places, cached forecasts and radar
//! maps are all named or keyed by coordinates. The default umask (022) would leave them
//! readable by other local users, so they are written owner-only.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

/// Writes `body` to `path` so only the owner can read it.
///
/// The data goes to a new private temporary file, which is then renamed over `path`. That
/// replaces the permissions of a file written by an older version, and a crash mid-write
/// never leaves a half-written file behind. A new parent directory is created `0700`; an
/// existing one keeps its permissions, since it may be the user's `~/.config` or similar.
pub fn write_private(path: &Path, body: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let temporary =
        path.with_file_name(format!("{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut file = private_file_options().open(&temporary)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Reads `path`, first tightening a file left group/world-readable by an older version to
/// owner-only. Writes already do this, but the normal run path only reads, so without it an
/// upgrade would leave old coordinates exposed until something happened to rewrite the file.
/// Tightening is best-effort: a failure to chmod never prevents the read.
pub fn read_private_to_string(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    tighten_open_file(&file);
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(raw)
}

/// Makes an existing file owner-only without reading it, for files a different program (the
/// QML shell) loads by path. Best-effort, like [`read_private_to_string`].
pub fn tighten_private(path: &Path) {
    if let Ok(file) = std::fs::File::open(path) {
        tighten_open_file(&file);
    }
}

/// Tightens every regular file under `dir`, recursively — a one-off sweep for files an older
/// version left world-readable and that may never be read again. Symlinks are skipped, so the
/// sweep cannot be steered at a file outside the tree. Directories keep their permissions.
pub fn tighten_private_tree(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            tighten_private_tree(&entry.path());
        } else if kind.is_file() {
            tighten_private(&entry.path());
        }
    }
}

#[cfg(unix)]
fn tighten_open_file(file: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = file.metadata() {
        if meta.permissions().mode() & 0o077 != 0 {
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn tighten_open_file(_file: &std::fs::File) {}

/// Create-new (never reuse an existing file, so nobody else's permissions carry over), 0600.
#[cfg(unix)]
fn private_file_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn reading_tightens_an_old_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rustybui-readpriv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_private_to_string(&path).unwrap(), "{}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn sweep_tightens_nested_files_and_skips_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = std::env::temp_dir().join(format!("rustybui-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("http/radar")).unwrap();
        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "x").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644)).unwrap();
        let nested = dir.join("http/radar/map.png");
        std::fs::write(&nested, "x").unwrap();
        std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o664)).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("http/link")).unwrap();
        tighten_private_tree(&dir.join("http"));
        assert_eq!(mode(&nested), 0o600);
        assert_eq!(mode(&outside), 0o644);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn new_files_are_owner_only_and_old_world_readable_ones_are_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rustybui-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cache").join("entry.json");

        write_private(&path, b"{}").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"{\"units\":\"imperial\"}").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"units\":\"imperial\"}");
        assert!(!dir.join("cache").join("entry.json.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
