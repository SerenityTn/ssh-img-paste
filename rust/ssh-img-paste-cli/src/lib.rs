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
