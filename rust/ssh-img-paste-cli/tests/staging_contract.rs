use ssh_img_paste_cli::{StageError, stage_upload_source};
use ssh_img_paste_core::CancellationToken;
use std::io::Read;
use std::path::PathBuf;

fn temporary_directory() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-staging-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).expect("staging test directory");
    root
}

struct CancellingReader {
    token: CancellationToken,
    reads: usize,
}

impl Read for CancellingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        if self.reads == 1 {
            let bytes = b"\x89PNG\r\n\x1a\nfirst staged bytes";
            buffer[..bytes.len()].copy_from_slice(bytes);
            return Ok(bytes.len());
        }
        self.token.cancel();
        let bytes = b"cancelled bytes";
        buffer[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
}

#[test]
fn cancellation_during_copy_removes_the_partially_staged_image() {
    let directory = temporary_directory();
    let token = CancellationToken::new();
    let mut reader = CancellingReader {
        token: token.clone(),
        reads: 0,
    };

    let result = stage_upload_source(&mut reader, &token, &directory);

    assert!(matches!(result, Err(StageError::Cancelled)));
    assert_eq!(
        std::fs::read_dir(&directory)
            .expect("list staging directory")
            .count(),
        0,
        "partial staging file was not removed"
    );
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn already_cancelled_upload_creates_no_staging_file() {
    let directory = temporary_directory();
    let token = CancellationToken::new();
    token.cancel();
    let mut bytes = std::io::Cursor::new(b"\x89PNG\r\n\x1a\nbytes");

    let result = stage_upload_source(&mut bytes, &token, &directory);

    assert!(matches!(result, Err(StageError::Cancelled)));
    assert_eq!(
        std::fs::read_dir(&directory)
            .expect("list staging directory")
            .count(),
        0
    );
    let _ = std::fs::remove_dir_all(directory);
}
