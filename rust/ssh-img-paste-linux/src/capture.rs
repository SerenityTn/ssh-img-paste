//! Region and full-screen capture.
//!
//! The XDG desktop portal comes first: it is the only route on GNOME and KDE
//! Wayland, and an interactive request opens the desktop's own area picker
//! (the Linux counterpart of `screencapture -i` on macOS). When no portal
//! answers, wlroots sessions use `grim` (with `slurp` for a region) and X11
//! sessions use `maim` or `scrot`. Tools always run from argument arrays.

use crate::clipboard::is_png;
use crate::process::{self, ProcessError, ProcessRequest};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Covers the user choosing a region; the portal has the same bound.
const SELECTION_TIMEOUT: Duration = Duration::from_secs(120);
const SHOT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_SHOT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Region,
    FullScreen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    Cancelled,
    /// Neither the portal nor a capture tool is available.
    Unavailable(String),
    Failed(String),
}

impl CaptureError {
    pub fn describe(&self) -> String {
        match self {
            Self::Cancelled => "Capture cancelled.".into(),
            Self::Unavailable(message) | Self::Failed(message) => message.clone(),
        }
    }
}

pub trait ScreenCapture {
    /// Blocks until the user finishes (or cancels) and returns PNG bytes.
    fn capture(&self, mode: CaptureMode) -> Result<Vec<u8>, CaptureError>;
}

/// A fallback capture program found on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTool {
    /// wlroots: `grim`, plus `slurp` to pick a region.
    Grim {
        grim: PathBuf,
        slurp: Option<PathBuf>,
    },
    Maim(PathBuf),
    Scrot(PathBuf),
}

impl CaptureTool {
    pub fn detect(env: impl Fn(&str) -> Option<OsString>) -> Option<Self> {
        let set = |name: &str| env(name).is_some_and(|value| !value.is_empty());
        let path = env("PATH");
        let find = |name: &str| process::find_on_path(name, path.as_deref());
        if set("WAYLAND_DISPLAY")
            && let Some(grim) = find("grim")
        {
            return Some(Self::Grim {
                grim,
                slurp: find("slurp"),
            });
        }
        if !set("DISPLAY") {
            return None;
        }
        find("maim")
            .map(Self::Maim)
            .or_else(|| find("scrot").map(Self::Scrot))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Grim { .. } => "grim",
            Self::Maim(_) => "maim",
            Self::Scrot(_) => "scrot",
        }
    }

    /// Runs the tool. `scratch` is a private directory for tools that can
    /// only write to a file.
    pub fn capture(&self, mode: CaptureMode, scratch: &Path) -> Result<Vec<u8>, CaptureError> {
        match (self, mode) {
            (Self::Grim { grim, .. }, CaptureMode::FullScreen) => {
                stdout_png(grim, &["-t", "png", "-"])
            }
            (Self::Grim { grim, slurp }, CaptureMode::Region) => {
                let slurp = slurp.as_ref().ok_or_else(|| {
                    CaptureError::Unavailable(
                        "Region capture needs slurp. Install it with \"sudo apt install slurp\"."
                            .into(),
                    )
                })?;
                let output = run(ProcessRequest::new(slurp, &[], SELECTION_TIMEOUT))?;
                if !output.status.success() {
                    return Err(CaptureError::Cancelled);
                }
                let geometry = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !valid_geometry(&geometry) {
                    return Err(CaptureError::Failed(
                        "slurp returned an invalid region.".into(),
                    ));
                }
                stdout_png(grim, &["-g", &geometry, "-t", "png", "-"])
            }
            (Self::Maim(maim), CaptureMode::FullScreen) => stdout_png(maim, &["--format", "png"]),
            (Self::Maim(maim), CaptureMode::Region) => {
                selection_png(maim, &["--select", "--format", "png"], None)
            }
            (Self::Scrot(scrot), mode) => {
                let target = scratch.join(format!("capture-{}.png", std::process::id()));
                let _ = std::fs::remove_file(&target);
                let target_text = target.to_str().ok_or_else(|| {
                    CaptureError::Failed("The capture folder path is not valid UTF-8.".into())
                })?;
                let result = match mode {
                    CaptureMode::FullScreen => {
                        let output = run(ProcessRequest::new(
                            scrot,
                            &["--overwrite", target_text],
                            SHOT_TIMEOUT,
                        ))?;
                        if output.status.success() {
                            Ok(Vec::new())
                        } else {
                            Err(tool_failure("scrot", &output.stderr))
                        }
                    }
                    CaptureMode::Region => selection_png(
                        scrot,
                        &["--select", "--overwrite", target_text],
                        Some(&target),
                    ),
                };
                let bytes = result.and_then(|_| {
                    std::fs::read(&target).map_err(|error| {
                        CaptureError::Failed(format!("The screenshot could not be read: {error}"))
                    })
                });
                let _ = std::fs::remove_file(&target);
                png_or_failure(bytes?)
            }
        }
    }
}

fn run(request: ProcessRequest) -> Result<process::ProcessOutput, CaptureError> {
    let mut request = request;
    request.max_output_bytes = MAX_SHOT_BYTES;
    process::run(&request).map_err(|error| match error {
        ProcessError::Timeout => {
            CaptureError::Failed("The capture took too long and was stopped.".into())
        }
        ProcessError::Spawn(error) | ProcessError::Wait(error) => {
            CaptureError::Failed(format!("The capture tool could not run: {error}"))
        }
    })
}

fn stdout_png(program: &Path, arguments: &[&str]) -> Result<Vec<u8>, CaptureError> {
    let output = run(ProcessRequest::new(program, arguments, SHOT_TIMEOUT))?;
    if output.stdout_truncated {
        return Err(CaptureError::Failed("The screenshot is too large.".into()));
    }
    if !output.status.success() {
        return Err(tool_failure(&program.display().to_string(), &output.stderr));
    }
    png_or_failure(output.stdout)
}

/// Interactive selection: a failed exit is a cancel unless the tool says
/// otherwise. With `file`, the image is written there instead of stdout.
fn selection_png(
    program: &Path,
    arguments: &[&str],
    file: Option<&Path>,
) -> Result<Vec<u8>, CaptureError> {
    let output = run(ProcessRequest::new(program, arguments, SELECTION_TIMEOUT))?;
    if !output.status.success() {
        return Err(if looks_cancelled(&output.stderr) {
            CaptureError::Cancelled
        } else {
            tool_failure(&program.display().to_string(), &output.stderr)
        });
    }
    match file {
        Some(_) => Ok(Vec::new()),
        None => png_or_failure(output.stdout),
    }
}

pub fn looks_cancelled(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    text.trim().is_empty()
        || ["cancel", "abort", "key was pressed", "giving up"]
            .iter()
            .any(|marker| text.contains(marker))
}

fn tool_failure(tool: &str, stderr: &[u8]) -> CaptureError {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    let name = Path::new(tool)
        .file_name()
        .map_or(tool.into(), |name| name.to_string_lossy().into_owned());
    CaptureError::Failed(if detail.is_empty() {
        format!("{name} could not take the screenshot.")
    } else {
        format!(
            "{name} could not take the screenshot: {}",
            truncate(detail, 200)
        )
    })
}

fn png_or_failure(bytes: Vec<u8>) -> Result<Vec<u8>, CaptureError> {
    if is_png(&bytes) {
        Ok(bytes)
    } else {
        Err(CaptureError::Failed(
            "The screenshot was not a PNG image.".into(),
        ))
    }
}

/// `slurp` prints `X,Y WxH`.
pub fn valid_geometry(value: &str) -> bool {
    let Some((origin, size)) = value.split_once(' ') else {
        return false;
    };
    let pair = |text: &str, separator: char, signed: bool| {
        text.split_once(separator).is_some_and(|(a, b)| {
            [a, b].iter().all(|part| {
                let digits = if signed {
                    part.strip_prefix('-').unwrap_or(part)
                } else {
                    part
                };
                !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
            })
        })
    };
    pair(origin, ',', true) && pair(size, 'x', false)
}

fn truncate(value: &str, length: usize) -> String {
    match value.char_indices().nth(length) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_owned(),
    }
}

/// Decodes a `file://` URI from the portal into a local path.
pub fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    // Only local files: the authority must be empty or localhost.
    let encoded = encoded.strip_prefix("localhost").unwrap_or(encoded);
    if !encoded.starts_with('/') {
        return None;
    }
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(OsString::from_vec(decoded)))
}

/// Portal first, then the session's capture tool.
pub struct SystemCapture {
    tool: Option<CaptureTool>,
    scratch: PathBuf,
    convert: Option<crate::clipboard::ImageConverter>,
}

impl SystemCapture {
    pub fn new(
        tool: Option<CaptureTool>,
        scratch: PathBuf,
        convert: Option<crate::clipboard::ImageConverter>,
    ) -> Self {
        Self {
            tool,
            scratch,
            convert,
        }
    }

    fn portal(&self, mode: CaptureMode) -> Result<Vec<u8>, PortalError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| PortalError::Unavailable(error.to_string()))?;
        let uri = runtime.block_on(async {
            use ashpd::desktop::ResponseError;
            use ashpd::desktop::screenshot::Screenshot;
            let request = Screenshot::request()
                .interactive(mode == CaptureMode::Region)
                .modal(true)
                .send()
                .await
                .map_err(|error| PortalError::Unavailable(error.to_string()))?;
            match request.response() {
                Ok(screenshot) => Ok(screenshot.uri().as_str().to_owned()),
                Err(ashpd::Error::Response(ResponseError::Cancelled)) => {
                    Err(PortalError::Cancelled)
                }
                Err(error) => Err(PortalError::Failed(error.to_string())),
            }
        })?;
        let path = file_uri_path(&uri).ok_or_else(|| {
            PortalError::Failed(format!("The portal returned an unusable location: {uri}"))
        })?;
        let bytes = std::fs::read(&path).map_err(|error| {
            PortalError::Failed(format!("The screenshot could not be read: {error}"))
        })?;
        if is_png(&bytes) {
            return Ok(bytes);
        }
        self.convert
            .and_then(|convert| convert(&bytes))
            .filter(|png| is_png(png))
            .ok_or_else(|| {
                PortalError::Failed("The screenshot could not be converted to PNG.".into())
            })
    }
}

enum PortalError {
    Unavailable(String),
    Cancelled,
    Failed(String),
}

impl ScreenCapture for SystemCapture {
    fn capture(&self, mode: CaptureMode) -> Result<Vec<u8>, CaptureError> {
        let portal_problem = match self.portal(mode) {
            Ok(png) => return Ok(png),
            Err(PortalError::Cancelled) => return Err(CaptureError::Cancelled),
            Err(PortalError::Failed(message)) if self.tool.is_none() => {
                return Err(CaptureError::Failed(format!(
                    "The screenshot portal failed: {message}"
                )));
            }
            Err(PortalError::Failed(message) | PortalError::Unavailable(message)) => message,
        };
        match &self.tool {
            Some(tool) => {
                std::fs::create_dir_all(&self.scratch).map_err(|error| {
                    CaptureError::Failed(format!(
                        "The capture folder could not be created: {error}"
                    ))
                })?;
                tool.capture(mode, &self.scratch)
            }
            None => Err(CaptureError::Unavailable(format!(
                "No screenshot service is available ({portal_problem}). Install \
                 xdg-desktop-portal for your desktop, or grim and slurp (wlroots), or maim (X11)."
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(names: &[&str]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("tempdir");
        for name in names {
            let path = directory.path().join(name);
            std::fs::write(&path, "#!/bin/sh\n").expect("write tool");
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .expect("chmod");
        }
        directory
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn prefers_grim_on_wayland_and_maim_over_scrot_on_x11() {
        let all = tools(&["grim", "slurp", "maim", "scrot"]);
        let path = all.path().to_str().expect("utf-8");
        let wayland = CaptureTool::detect(env(&[
            ("WAYLAND_DISPLAY", "w"),
            ("DISPLAY", ":0"),
            ("PATH", path),
        ]));
        assert!(matches!(
            wayland,
            Some(CaptureTool::Grim { slurp: Some(_), .. })
        ));
        let x11 = CaptureTool::detect(env(&[("DISPLAY", ":0"), ("PATH", path)]));
        assert_eq!(x11.map(|tool| tool.name()), Some("maim"));

        let scrot_only = tools(&["scrot"]);
        let path = scrot_only.path().to_str().expect("utf-8");
        let x11 = CaptureTool::detect(env(&[("DISPLAY", ":0"), ("PATH", path)]));
        assert_eq!(x11.map(|tool| tool.name()), Some("scrot"));
        // GNOME Wayland without grim relies on the portal alone.
        assert_eq!(
            CaptureTool::detect(env(&[("WAYLAND_DISPLAY", "w"), ("PATH", path)])),
            None
        );
    }

    #[test]
    fn decodes_local_file_uris_only() {
        assert_eq!(
            file_uri_path("file:///home/me/Pictures/Screenshot%20from%202026.png"),
            Some(PathBuf::from("/home/me/Pictures/Screenshot from 2026.png"))
        );
        assert_eq!(
            file_uri_path("file://localhost/tmp/a.png"),
            Some(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(file_uri_path("file://server/share/a.png"), None);
        assert_eq!(file_uri_path("https://example.com/a.png"), None);
        assert_eq!(file_uri_path("file:///bad%zz"), None);
    }

    #[test]
    fn validates_slurp_geometry() {
        assert!(valid_geometry("10,20 300x200"));
        assert!(valid_geometry("-1920,0 1920x1080"));
        assert!(!valid_geometry("10,20 300x"));
        assert!(!valid_geometry("10,20;rm 300x200"));
        assert!(!valid_geometry(""));
    }

    #[test]
    fn classifies_selection_exits() {
        assert!(looks_cancelled(b""));
        assert!(looks_cancelled(
            b"Selection was cancelled by keystroke or right-click."
        ));
        assert!(looks_cancelled(
            b"scrot: Key was pressed, aborting selection"
        ));
        assert!(!looks_cancelled(
            b"Can't open X display. It *is* running, yeah?"
        ));
    }

    #[test]
    fn scrot_full_screen_reads_and_removes_its_file() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let bin = tempfile::tempdir().expect("tempdir");
        let scrot = bin.path().join("scrot");
        // Fake scrot: writes a PNG signature to the path after --overwrite.
        std::fs::write(
            &scrot,
            "#!/bin/sh\nprintf '\\211PNG\\r\\n\\032\\nfake' > \"$2\"\n",
        )
        .expect("write");
        std::fs::set_permissions(&scrot, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let png = CaptureTool::Scrot(scrot)
            .capture(CaptureMode::FullScreen, scratch.path())
            .expect("capture");
        assert!(is_png(&png));
        assert_eq!(
            std::fs::read_dir(scratch.path()).expect("scratch").count(),
            0
        );
    }

    #[test]
    fn a_cancelled_selection_is_not_a_failure() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let bin = tempfile::tempdir().expect("tempdir");
        let maim = bin.path().join("maim");
        std::fs::write(
            &maim,
            "#!/bin/sh\necho 'Selection was cancelled by keystroke or right-click.' >&2\nexit 1\n",
        )
        .expect("write");
        std::fs::set_permissions(&maim, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        assert_eq!(
            CaptureTool::Maim(maim).capture(CaptureMode::Region, scratch.path()),
            Err(CaptureError::Cancelled)
        );
    }
}
