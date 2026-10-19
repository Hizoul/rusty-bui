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
