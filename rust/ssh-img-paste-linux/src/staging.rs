//! Private, uniquely named PNG files handed to the CLI.
//!
//! The CLI copies the bytes into its own staging before any network step, so
//! a file only lives for one upload. The directory prefers
//! `$XDG_RUNTIME_DIR` (per-user tmpfs) and falls back to the XDG cache.

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const APP_DIRECTORY: &str = "ssh-img-paste";
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug)]
pub struct StagingArea {
    directory: PathBuf,
}

#[derive(Debug)]
pub struct StagedFile {
    path: PathBuf,
}

impl StagedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl StagingArea {
    /// Resolves the staging directory from absolute XDG locations only.
    pub fn locate(env: impl Fn(&str) -> Option<OsString>) -> Option<Self> {
        let absolute = |name: &str| {
            env(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        let base = absolute("XDG_RUNTIME_DIR")
            .or_else(|| absolute("XDG_CACHE_HOME"))
            .or_else(|| absolute("HOME").map(|home| home.join(".cache")))?;
        Some(Self::at(base.join(APP_DIRECTORY).join("staging")))
    }

    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn write(&self, png: &[u8]) -> std::io::Result<StagedFile> {
        self.prepare()?;
        let path = self
            .directory
            .join(format!("upload-{}.png", random_hex(12)?));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let staged = StagedFile { path };
        file.write_all(png)?;
        file.sync_all()?;
        Ok(staged)
    }

    /// Removes files left behind by a crash or forced shutdown.
    pub fn remove_stale(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("upload-") && name.ends_with(".png")) {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified).unwrap_or_default() > STALE_AFTER
                });
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Creates the directory as 0700 and refuses one that is a link, not a
    /// directory, or owned by someone else.
    fn prepare(&self) -> std::io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        let metadata = fs::symlink_metadata(&self.directory)?;
        // SAFETY: `getuid` has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        if !metadata.is_dir() || metadata.uid() != uid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the staging directory is not a private directory",
            ));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut buffer = vec![0_u8; bytes];
    File::open("/dev/urandom")?.read_exact(&mut buffer)?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_runtime_directory_and_ignores_relative_paths() {
        let env = |name: &str| match name {
            "XDG_RUNTIME_DIR" => Some(OsString::from("relative")),
            "XDG_CACHE_HOME" => Some(OsString::from("/home/me/.cache-alt")),
            _ => None,
        };
        let area = StagingArea::locate(env).expect("staging");
        assert_eq!(
            area.directory(),
            Path::new("/home/me/.cache-alt/ssh-img-paste/staging")
        );
        let runtime = StagingArea::locate(|name| {
            (name == "XDG_RUNTIME_DIR").then(|| OsString::from("/run/user/1000"))
        })
        .expect("staging");
        assert_eq!(
            runtime.directory(),
            Path::new("/run/user/1000/ssh-img-paste/staging")
        );
        assert!(StagingArea::locate(|_| None).is_none());
    }

    #[test]
    fn staged_files_are_private_unique_and_removed_on_drop() {
        let root = tempfile::tempdir().expect("tempdir");
        let area = StagingArea::at(root.path().join("ssh-img-paste/staging"));
        let first = area.write(b"\x89PNG\r\n\x1a\none").expect("write");
        let second = area.write(b"\x89PNG\r\n\x1a\ntwo").expect("write");
        assert_ne!(first.path(), second.path());
        assert_eq!(
            fs::read(first.path()).expect("read"),
            b"\x89PNG\r\n\x1a\none"
        );
        let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(first.path()), 0o600);
        assert_eq!(mode(area.directory()), 0o700);
        let path = first.path().to_owned();
        drop(first);
        assert!(!path.exists());
    }

    #[test]
    fn refuses_a_symlinked_staging_directory() {
        let root = tempfile::tempdir().expect("tempdir");
        let target = root.path().join("elsewhere");
        fs::create_dir(&target).expect("mkdir");
        let link = root.path().join("staging");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let error = StagingArea::at(link)
            .write(b"png")
            .expect_err("link refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn stale_cleanup_keeps_fresh_and_foreign_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let area = StagingArea::at(root.path().to_owned());
        let fresh = area.write(b"png").expect("write");
        let foreign = root.path().join("notes.txt");
        fs::write(&foreign, "keep").expect("write");
        area.remove_stale();
        assert!(fresh.path().exists());
        assert!(foreign.exists());
    }
}
