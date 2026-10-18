use ssh_img_paste_core::CancellationToken;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageError {
    Cancelled,
    Io(std::io::ErrorKind),
}

pub fn stage_upload_source(
    source: &mut impl Read,
    cancellation: &CancellationToken,
    directory: &Path,
) -> Result<tempfile::NamedTempFile, StageError> {
    if cancellation.is_cancelled() {
        return Err(StageError::Cancelled);
    }
    let mut staged = tempfile::Builder::new()
        .prefix("ssh-img-paste-")
        .suffix(".png")
        .tempfile_in(directory)
        .map_err(|error| StageError::Io(error.kind()))?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(StageError::Cancelled);
        }
        let count = source
            .read(&mut buffer)
            .map_err(|error| StageError::Io(error.kind()))?;
        if cancellation.is_cancelled() {
            return Err(StageError::Cancelled);
        }
        if count == 0 {
            break;
        }
        staged
            .write_all(&buffer[..count])
            .map_err(|error| StageError::Io(error.kind()))?;
    }
    if cancellation.is_cancelled() {
        return Err(StageError::Cancelled);
    }
    staged
        .as_file_mut()
        .sync_all()
        .map_err(|error| StageError::Io(error.kind()))?;
    if cancellation.is_cancelled() {
        return Err(StageError::Cancelled);
    }
    Ok(staged)
}

const DOWNLOADS_DIRECTORY: &str = "ssh-img-paste-downloads";
const DOWNLOAD_RETENTION: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// Creates a private, uniquely named directory for one fetched upload.
///
/// Download directories older than a week are removed on a best-effort
/// basis, so opened images do not accumulate in the temporary directory.
pub fn prepare_download_directory(temp_root: &Path) -> std::io::Result<tempfile::TempDir> {
    let base = temp_root.join(DOWNLOADS_DIRECTORY);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&base)?;
    let metadata = std::fs::symlink_metadata(&base)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "download directory is not a plain directory",
        ));
    }
    remove_stale_downloads(&base);
    tempfile::Builder::new().prefix("fetch-").tempdir_in(&base)
}

fn remove_stale_downloads(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > DOWNLOAD_RETENTION);
        if stale && metadata.is_dir() && entry.file_name().to_string_lossy().starts_with("fetch-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
