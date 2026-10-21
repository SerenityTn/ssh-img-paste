//! The clipboard upload workflow: read a PNG from the clipboard, hand a
//! private copy to the CLI, then place the remote path on the clipboard.
//!
//! Runs on a worker thread; every step blocks on a bounded child process.

use crate::cli::{self, CliRunner};
use crate::clipboard::ClipboardAccess;
use crate::messages;
use crate::staging::StagingArea;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded {
        remote_path: String,
        destination: String,
        /// False when the upload worked but the clipboard write did not.
        path_copied: bool,
    },
    NoDestination,
    NoImage {
        destination: String,
    },
    Failed(String),
}

impl UploadOutcome {
    pub fn summary(&self) -> String {
        match self {
            Self::Uploaded {
                path_copied: true, ..
            } => "Remote path copied to the clipboard.".into(),
            Self::Uploaded {
                path_copied: false, ..
            } => "Uploaded, but the clipboard could not be updated. Copy the path from the window."
                .into(),
            Self::NoDestination => "Add an SSH destination before uploading. Run \
                                    \"ssh-img-paste profiles create\" in a terminal."
                .into(),
            Self::NoImage { .. } => "No image found on the clipboard. Copy an image first.".into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

pub fn upload_clipboard<C: ClipboardAccess>(
    runner: &impl CliRunner,
    clipboard: Result<&C, &crate::clipboard::ClipboardError>,
    staging: &StagingArea,
) -> UploadOutcome {
    let catalog = match cli::catalog(runner) {
        Ok(catalog) => catalog,
        Err(error) if error.code == "not_configured" => return UploadOutcome::NoDestination,
        Err(error) => return UploadOutcome::Failed(messages::describe(&error)),
    };
    let Some(profile) = catalog.active() else {
        return UploadOutcome::NoDestination;
    };
    let clipboard = match clipboard {
        Ok(clipboard) => clipboard,
        Err(error) => return UploadOutcome::Failed(error.describe()),
    };
    let png = match clipboard.read_png() {
        Ok(Some(png)) => png,
        Ok(None) => {
            return UploadOutcome::NoImage {
                destination: profile.label.clone(),
            };
        }
        Err(error) => return UploadOutcome::Failed(error.describe()),
    };
    let staged = match staging.write(&png) {
        Ok(staged) => staged,
        Err(error) => {
            return UploadOutcome::Failed(format!("The image could not be staged: {error}"));
        }
    };
    let remote_path = match cli::upload_file(runner, staged.path(), &profile.id) {
        Ok(remote_path) => remote_path,
        Err(error) => return UploadOutcome::Failed(messages::describe(&error)),
    };
    drop(staged);
    UploadOutcome::Uploaded {
        path_copied: clipboard.set_text(&remote_path).is_ok(),
        remote_path,
        destination: profile.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliError;
    use crate::clipboard::ClipboardError;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::time::Duration;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage";

    #[derive(Default)]
    struct FakeCli {
        catalog: Option<Result<Value, CliError>>,
        upload: Option<Result<Value, CliError>>,
        staged: RefCell<Option<(PathBuf, Vec<u8>)>>,
    }

    impl CliRunner for FakeCli {
        fn run(&self, arguments: &[&str], _timeout: Duration) -> Result<Value, CliError> {
            match arguments[0] {
                "profiles" => self.catalog.clone().expect("catalog requested"),
                "upload-file" => {
                    let source = PathBuf::from(arguments[2]);
                    let bytes = std::fs::read(&source).expect("staged file exists during upload");
                    *self.staged.borrow_mut() = Some((source, bytes));
                    self.upload.clone().expect("upload requested")
                }
                other => panic!("unexpected command {other}"),
            }
        }
    }

    struct FakeClipboard {
        image: Result<Option<Vec<u8>>, ClipboardError>,
        write_ok: bool,
        written: RefCell<Option<String>>,
    }

    impl FakeClipboard {
        fn with(image: Option<&[u8]>) -> Self {
            Self {
                image: Ok(image.map(<[u8]>::to_vec)),
                write_ok: true,
                written: RefCell::new(None),
            }
        }
    }

    impl ClipboardAccess for FakeClipboard {
        fn read_png(&self) -> Result<Option<Vec<u8>>, ClipboardError> {
            self.image.clone()
        }
        fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
            *self.written.borrow_mut() = Some(text.to_owned());
            if self.write_ok {
                Ok(())
            } else {
                Err(ClipboardError::Tool("busy".into()))
            }
        }
    }

    fn configured() -> Result<Value, CliError> {
        Ok(json!({
            "configured": true,
            "active_profile_id": "work",
            "profiles": [{"id": "work", "usable": true, "active": true, "label": "Work VPS"}]
        }))
    }

    fn staging() -> (tempfile::TempDir, StagingArea) {
        let root = tempfile::tempdir().expect("tempdir");
        let area = StagingArea::at(root.path().join("staging"));
        (root, area)
    }

    #[test]
    fn uploads_the_clipboard_png_and_copies_the_remote_path() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(configured()),
            upload: Some(Ok(json!({"remote_path": "/home/me/img-uploads/a.png"}))),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(Some(PNG));
        let outcome = upload_clipboard(&cli, Ok(&clipboard), &area);
        assert_eq!(
            outcome,
            UploadOutcome::Uploaded {
                remote_path: "/home/me/img-uploads/a.png".into(),
                destination: "Work VPS".into(),
                path_copied: true,
            }
        );
        assert_eq!(
            clipboard.written.borrow().as_deref(),
            Some("/home/me/img-uploads/a.png")
        );
        let (source, bytes) = cli.staged.borrow().clone().expect("uploaded");
        assert_eq!(bytes, PNG);
        assert!(!source.exists(), "staging file is removed after the upload");
    }

    #[test]
    fn reports_a_missing_destination_before_touching_the_clipboard() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(Err(CliError::new("not_configured", "No profiles."))),
            ..Default::default()
        };
        let clipboard = FakeClipboard {
            image: Err(ClipboardError::NoDisplay),
            write_ok: true,
            written: RefCell::new(None),
        };
        assert_eq!(
            upload_clipboard(&cli, Ok(&clipboard), &area),
            UploadOutcome::NoDestination
        );
    }

    #[test]
    fn empty_clipboard_is_not_an_error() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(configured()),
            ..Default::default()
        };
        let outcome = upload_clipboard(&cli, Ok(&FakeClipboard::with(None)), &area);
        assert!(matches!(outcome, UploadOutcome::NoImage { .. }));
    }

    #[test]
    fn cli_failures_become_actionable_messages_and_clean_up() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(configured()),
            upload: Some(Err(CliError {
                diagnostic: Some("Permission denied (publickey).".into()),
                ..CliError::new("upload_failed", "The upload failed.")
            })),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(Some(PNG));
        let UploadOutcome::Failed(message) = upload_clipboard(&cli, Ok(&clipboard), &area) else {
            panic!("expected failure");
        };
        assert!(message.contains("ssh-add"));
        assert!(clipboard.written.borrow().is_none());
        let (source, _) = cli.staged.borrow().clone().expect("attempted");
        assert!(!source.exists());
    }

    #[test]
    fn clipboard_write_failure_still_reports_the_upload() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(configured()),
            upload: Some(Ok(json!({"remote_path": "/r/a.png"}))),
            ..Default::default()
        };
        let mut clipboard = FakeClipboard::with(Some(PNG));
        clipboard.write_ok = false;
        let outcome = upload_clipboard(&cli, Ok(&clipboard), &area);
        assert!(matches!(
            outcome,
            UploadOutcome::Uploaded {
                path_copied: false,
                ..
            }
        ));
        assert!(outcome.summary().contains("Copy the path"));
    }

    #[test]
    fn unavailable_clipboard_explains_what_to_install() {
        let (_root, area) = staging();
        let cli = FakeCli {
            catalog: Some(configured()),
            ..Default::default()
        };
        let missing = ClipboardError::MissingTool {
            backend: crate::clipboard::Backend::X11,
        };
        let outcome = upload_clipboard::<FakeClipboard>(&cli, Err(&missing), &area);
        assert!(matches!(outcome, UploadOutcome::Failed(message) if message.contains("xclip")));
    }
}
