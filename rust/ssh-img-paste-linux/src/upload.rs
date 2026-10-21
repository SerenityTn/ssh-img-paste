//! The upload workflow, as in the Windows edition: acquire an image (the
//! clipboard, else the destination's screenshot fallback), hand a private
//! PNG to the CLI, then place the remote path on the clipboard and arrange
//! for the earlier clipboard to return after the destination's delay.
//!
//! Runs on a worker thread; every step blocks on a bounded child process.

use crate::capture::{CaptureError, CaptureMode, ScreenCapture};
use crate::cli::{self, CliRunner, ShotMode};
use crate::clipboard::{ClipboardAccess, ClipboardClaim, ClipboardError, ClipboardSnapshot};
use crate::messages;
use crate::staging::StagingArea;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadSource {
    /// The clipboard image, else the destination's screenshot fallback.
    Auto,
    Region,
    FullScreen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    Uploaded {
        remote_path: String,
        destination: String,
        from_screenshot: bool,
        /// False when the upload worked but the clipboard write did not.
        path_copied: bool,
        /// Seconds until the earlier clipboard returns; 0 keeps the path.
        restore_seconds: u32,
    },
    NoDestination,
    Cancelled,
    Failed(String),
}

impl UploadOutcome {
    /// `Image path copied — Work` or `Screenshot path copied — Work`.
    pub fn title(&self) -> String {
        match self {
            Self::Uploaded {
                destination,
                from_screenshot,
                ..
            } => format!(
                "{} path copied — {destination}",
                if *from_screenshot {
                    "Screenshot"
                } else {
                    "Image"
                }
            ),
            Self::NoDestination => "No destination".into(),
            Self::Cancelled => "Capture cancelled".into(),
            Self::Failed(_) => "Upload failed".into(),
        }
    }

    pub fn restore_line(&self) -> String {
        match self {
            Self::Uploaded {
                restore_seconds, ..
            } if *restore_seconds > 0 => {
                format!("The previous clipboard returns in {restore_seconds}s.")
            }
            _ => "The path stays on the clipboard.".into(),
        }
    }

    /// The status line shown in the main window.
    pub fn summary(&self) -> String {
        match self {
            Self::Uploaded {
                path_copied: false, ..
            } => "Uploaded, but the clipboard was busy. Copy the path from the window.".into(),
            Self::Uploaded { .. } => format!("{}. {}", self.title(), self.restore_line()),
            Self::NoDestination => "Add an SSH destination before uploading.".into(),
            Self::Cancelled => "Capture cancelled.".into(),
            Self::Failed(message) => message.clone(),
        }
    }

    /// Notification body: the path and restore line, or the failure.
    pub fn notification_body(&self) -> String {
        match self {
            Self::Uploaded {
                remote_path,
                path_copied: true,
                ..
            } => format!("{remote_path}\n{}", self.restore_line()),
            other => other.summary(),
        }
    }
}

/// Restore the earlier clipboard after `after`, if the path is still ours.
#[derive(Debug)]
pub struct PendingRestore {
    pub after: Duration,
    pub expected_text: String,
    pub snapshot: ClipboardSnapshot,
    pub claim: ClipboardClaim,
}

impl PendingRestore {
    /// Restores only while this app still owns the clipboard and it still
    /// holds the path; anything another app copied meanwhile wins.
    pub fn complete(mut self, clipboard: &impl ClipboardAccess) -> Result<bool, ClipboardError> {
        if !self.claim.still_owned()
            || clipboard.read_text().as_deref() != Some(self.expected_text.as_str())
        {
            return Ok(false);
        }
        clipboard.restore(&self.snapshot).map(|_| true)
    }
}

/// Callbacks into the desktop shell while an upload runs.
pub trait UploadHooks {
    fn progress(&self, message: &str);
    /// Called right before a screen capture so app windows can hide.
    fn before_capture(&self);
}

pub struct UploadRequest<'a, R, C, S> {
    pub source: UploadSource,
    pub runner: &'a R,
    pub clipboard: Result<&'a C, &'a ClipboardError>,
    pub capture: &'a S,
    pub staging: &'a StagingArea,
}

pub fn run_upload<R: CliRunner, C: ClipboardAccess, S: ScreenCapture>(
    request: UploadRequest<'_, R, C, S>,
    hooks: &impl UploadHooks,
) -> (UploadOutcome, Option<PendingRestore>) {
    let failed = |message: String| (UploadOutcome::Failed(message), None);
    hooks.progress("Checking destination…");
    let catalog = match cli::catalog(request.runner) {
        Ok(catalog) => catalog,
        Err(error) if error.code == "not_configured" => {
            return (UploadOutcome::NoDestination, None);
        }
        Err(error) => return failed(messages::describe(&error)),
    };
    let Some(profile) = catalog.active() else {
        return (UploadOutcome::NoDestination, None);
    };
    let clipboard = request.clipboard.ok();

    let mut mode = match request.source {
        UploadSource::Region => Some(CaptureMode::Region),
        UploadSource::FullScreen => Some(CaptureMode::FullScreen),
        UploadSource::Auto => None,
    };
    let mut png = None;
    if mode.is_none() {
        // Without clipboard tools, Auto still works through a capture.
        if let Ok(clipboard) = request.clipboard {
            match clipboard.read_png() {
                Ok(Some(image)) => png = Some(image),
                Ok(None) => {}
                Err(error) => return failed(error.describe()),
            }
        }
        if png.is_none() {
            mode = Some(match profile.shot_mode {
                ShotMode::Full => CaptureMode::FullScreen,
                ShotMode::Region => CaptureMode::Region,
            });
        }
    }
    let from_screenshot = mode.is_some();
    if let Some(mode) = mode {
        hooks.before_capture();
        match request.capture.capture(mode) {
            Ok(image) => png = Some(image),
            Err(CaptureError::Cancelled) => return (UploadOutcome::Cancelled, None),
            Err(error) => return failed(error.describe()),
        }
    }
    let png = png.expect("an image was acquired");

    let staged = match request.staging.write(&png) {
        Ok(staged) => staged,
        Err(error) => return failed(format!("The image could not be staged: {error}")),
    };
    hooks.progress(&format!("Uploading to {}…", profile.label));
    let remote_path = match cli::upload_file(request.runner, staged.path(), &profile.id) {
        Ok(remote_path) => remote_path,
        Err(error) => return failed(messages::describe(&error)),
    };
    drop(staged);

    let Some(clipboard) = clipboard else {
        if let Err(error) = request.clipboard {
            eprintln!("ssh-img-paste: {}", error.describe());
        }
        return (
            UploadOutcome::Uploaded {
                remote_path,
                destination: profile.label.clone(),
                from_screenshot,
                path_copied: false,
                restore_seconds: 0,
            },
            None,
        );
    };
    let snapshot = (profile.restore_seconds > 0)
        .then(|| clipboard.snapshot())
        .flatten();
    let (path_copied, restore) = match clipboard.set_text(&remote_path) {
        Ok(claim) => (
            true,
            snapshot.map(|snapshot| PendingRestore {
                after: Duration::from_secs(profile.restore_seconds.into()),
                expected_text: remote_path.clone(),
                snapshot,
                claim,
            }),
        ),
        Err(_) => (false, None),
    };
    (
        UploadOutcome::Uploaded {
            restore_seconds: if restore.is_some() {
                profile.restore_seconds
            } else {
                0
            },
            remote_path,
            destination: profile.label.clone(),
            from_screenshot,
            path_copied,
        },
        restore,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CliError;
    use serde_json::{Value, json};
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage";
    const SHOT: &[u8] = b"\x89PNG\r\n\x1a\nscreenshot";

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
        previous: Option<ClipboardSnapshot>,
        written: RefCell<Option<String>>,
    }

    impl FakeClipboard {
        fn with(image: Option<&[u8]>) -> Self {
            Self {
                image: Ok(image.map(<[u8]>::to_vec)),
                write_ok: true,
                previous: Some(ClipboardSnapshot {
                    mime: "image/png".into(),
                    bytes: PNG.to_vec(),
                }),
                written: RefCell::new(None),
            }
        }
    }

    impl ClipboardAccess for FakeClipboard {
        fn read_png(&self) -> Result<Option<Vec<u8>>, ClipboardError> {
            self.image.clone()
        }
        fn set_text(&self, text: &str) -> Result<ClipboardClaim, ClipboardError> {
            *self.written.borrow_mut() = Some(text.to_owned());
            if self.write_ok {
                Ok(ClipboardClaim::untracked())
            } else {
                Err(ClipboardError::Tool("busy".into()))
            }
        }
        fn snapshot(&self) -> Option<ClipboardSnapshot> {
            self.previous.clone()
        }
        fn restore(&self, _: &ClipboardSnapshot) -> Result<ClipboardClaim, ClipboardError> {
            Ok(ClipboardClaim::untracked())
        }
        fn read_text(&self) -> Option<String> {
            self.written.borrow().clone()
        }
    }

    struct FakeCapture {
        result: Result<Vec<u8>, CaptureError>,
        requested: Cell<Option<CaptureMode>>,
    }

    impl FakeCapture {
        fn returning(result: Result<Vec<u8>, CaptureError>) -> Self {
            Self {
                result,
                requested: Cell::new(None),
            }
        }
    }

    impl ScreenCapture for FakeCapture {
        fn capture(&self, mode: CaptureMode) -> Result<Vec<u8>, CaptureError> {
            self.requested.set(Some(mode));
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct Hooks {
        progress: RefCell<Vec<String>>,
        hid: Cell<bool>,
    }

    impl UploadHooks for Hooks {
        fn progress(&self, message: &str) {
            self.progress.borrow_mut().push(message.to_owned());
        }
        fn before_capture(&self) {
            self.hid.set(true);
        }
    }

    fn configured(shot_mode: &str, restore: &str) -> Result<Value, CliError> {
        Ok(json!({
            "configured": true,
            "active_profile_id": "work",
            "profiles": [{"id": "work", "usable": true, "active": true, "label": "Work VPS",
                          "shot_mode": shot_mode, "restore_seconds": restore}]
        }))
    }

    fn staging() -> (tempfile::TempDir, StagingArea) {
        let root = tempfile::tempdir().expect("tempdir");
        let area = StagingArea::at(root.path().join("staging"));
        (root, area)
    }

    fn uploaded() -> Option<Result<Value, CliError>> {
        Some(Ok(json!({"remote_path": "/home/me/img-uploads/a.png"})))
    }

    fn run(
        source: UploadSource,
        cli: &FakeCli,
        clipboard: &FakeClipboard,
        capture: &FakeCapture,
        hooks: &Hooks,
    ) -> (UploadOutcome, Option<PendingRestore>) {
        let (_root, area) = staging();
        run_upload(
            UploadRequest {
                source,
                runner: cli,
                clipboard: Ok(clipboard),
                capture,
                staging: &area,
            },
            hooks,
        )
    }

    #[test]
    fn auto_prefers_the_clipboard_image_and_schedules_a_restore() {
        let cli = FakeCli {
            catalog: Some(configured("region", "15")),
            upload: uploaded(),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(Some(PNG));
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let hooks = Hooks::default();
        let (outcome, restore) = run(UploadSource::Auto, &cli, &clipboard, &capture, &hooks);
        assert_eq!(outcome.title(), "Image path copied — Work VPS");
        assert_eq!(
            outcome.summary(),
            "Image path copied — Work VPS. The previous clipboard returns in 15s."
        );
        assert_eq!(
            outcome.notification_body(),
            "/home/me/img-uploads/a.png\nThe previous clipboard returns in 15s."
        );
        assert_eq!(capture.requested.get(), None);
        assert!(!hooks.hid.get());
        assert_eq!(
            cli.staged.borrow().as_ref().map(|(_, b)| b.clone()),
            Some(PNG.to_vec())
        );
        let restore = restore.expect("restore scheduled");
        assert_eq!(restore.after, Duration::from_secs(15));
        assert_eq!(
            hooks.progress.borrow().as_slice(),
            ["Checking destination…", "Uploading to Work VPS…"]
        );
    }

    #[test]
    fn auto_without_an_image_captures_with_the_profile_fallback() {
        let cli = FakeCli {
            catalog: Some(configured("full", "0")),
            upload: uploaded(),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(None);
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let hooks = Hooks::default();
        let (outcome, restore) = run(UploadSource::Auto, &cli, &clipboard, &capture, &hooks);
        assert_eq!(capture.requested.get(), Some(CaptureMode::FullScreen));
        assert!(hooks.hid.get());
        assert_eq!(outcome.title(), "Screenshot path copied — Work VPS");
        assert_eq!(outcome.restore_line(), "The path stays on the clipboard.");
        assert!(restore.is_none(), "restore_seconds 0 keeps the path");
        assert_eq!(
            cli.staged.borrow().as_ref().map(|(_, b)| b.clone()),
            Some(SHOT.to_vec())
        );
    }

    #[test]
    fn explicit_region_capture_skips_the_clipboard() {
        let cli = FakeCli {
            catalog: Some(configured("full", "60")),
            upload: uploaded(),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(Some(PNG));
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let (outcome, _) = run(
            UploadSource::Region,
            &cli,
            &clipboard,
            &capture,
            &Hooks::default(),
        );
        assert_eq!(capture.requested.get(), Some(CaptureMode::Region));
        assert!(matches!(
            outcome,
            UploadOutcome::Uploaded {
                from_screenshot: true,
                ..
            }
        ));
    }

    #[test]
    fn cancelled_capture_uploads_nothing() {
        let cli = FakeCli {
            catalog: Some(configured("region", "60")),
            ..Default::default()
        };
        let capture = FakeCapture::returning(Err(CaptureError::Cancelled));
        let (outcome, _) = run(
            UploadSource::Region,
            &cli,
            &FakeClipboard::with(None),
            &capture,
            &Hooks::default(),
        );
        assert_eq!(outcome, UploadOutcome::Cancelled);
        assert_eq!(outcome.summary(), "Capture cancelled.");
        assert!(cli.staged.borrow().is_none());
    }

    #[test]
    fn missing_destination_stops_before_any_capture() {
        let cli = FakeCli {
            catalog: Some(Err(CliError::new("not_configured", "No profiles."))),
            ..Default::default()
        };
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let (outcome, _) = run(
            UploadSource::FullScreen,
            &cli,
            &FakeClipboard::with(None),
            &capture,
            &Hooks::default(),
        );
        assert_eq!(outcome, UploadOutcome::NoDestination);
        assert_eq!(capture.requested.get(), None);
    }

    #[test]
    fn cli_failures_become_actionable_messages_and_clean_up() {
        let cli = FakeCli {
            catalog: Some(configured("region", "60")),
            upload: Some(Err(CliError {
                diagnostic: Some("Permission denied (publickey).".into()),
                ..CliError::new("upload_failed", "The upload failed.")
            })),
            ..Default::default()
        };
        let clipboard = FakeClipboard::with(Some(PNG));
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let (outcome, _) = run(
            UploadSource::Auto,
            &cli,
            &clipboard,
            &capture,
            &Hooks::default(),
        );
        let UploadOutcome::Failed(message) = &outcome else {
            panic!("expected failure");
        };
        assert!(message.contains("ssh-add"));
        assert_eq!(outcome.title(), "Upload failed");
        assert!(clipboard.written.borrow().is_none());
        let (source, _) = cli.staged.borrow().clone().expect("attempted");
        assert!(!source.exists());
    }

    #[test]
    fn busy_clipboard_still_reports_the_upload() {
        let cli = FakeCli {
            catalog: Some(configured("region", "60")),
            upload: uploaded(),
            ..Default::default()
        };
        let mut clipboard = FakeClipboard::with(Some(PNG));
        clipboard.write_ok = false;
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let (outcome, restore) = run(
            UploadSource::Auto,
            &cli,
            &clipboard,
            &capture,
            &Hooks::default(),
        );
        assert!(matches!(
            outcome,
            UploadOutcome::Uploaded {
                path_copied: false,
                ..
            }
        ));
        assert!(restore.is_none());
        assert!(outcome.summary().contains("Copy the path from the window"));
    }

    #[test]
    fn missing_clipboard_tools_still_allow_capture_uploads() {
        let cli = FakeCli {
            catalog: Some(configured("region", "60")),
            upload: uploaded(),
            ..Default::default()
        };
        let capture = FakeCapture::returning(Ok(SHOT.to_vec()));
        let missing = ClipboardError::MissingTool {
            backend: crate::clipboard::Backend::X11,
        };
        let (_root, area) = staging();
        let (outcome, _) = run_upload::<_, FakeClipboard, _>(
            UploadRequest {
                source: UploadSource::Auto,
                runner: &cli,
                clipboard: Err(&missing),
                capture: &capture,
                staging: &area,
            },
            &Hooks::default(),
        );
        assert!(matches!(
            outcome,
            UploadOutcome::Uploaded {
                path_copied: false,
                from_screenshot: true,
                ..
            }
        ));
    }

    #[test]
    fn restore_is_skipped_once_the_clipboard_changed() {
        let clipboard = FakeClipboard::with(None);
        *clipboard.written.borrow_mut() = Some("something else".into());
        let pending = PendingRestore {
            after: Duration::ZERO,
            expected_text: "/home/me/img-uploads/a.png".into(),
            snapshot: ClipboardSnapshot {
                mime: "image/png".into(),
                bytes: PNG.to_vec(),
            },
            claim: ClipboardClaim::untracked(),
        };
        assert_eq!(pending.complete(&clipboard), Ok(false));
    }
}
