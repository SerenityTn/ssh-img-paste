//! Clipboard access through `wl-clipboard` on Wayland and `xclip` on X11.
//!
//! A tray application rarely has keyboard focus, and Wayland compositors only
//! hand the selection to focused clients, so the toolkit clipboard is not
//! reliable here. The external tools work around that and keep serving the
//! copied path after this app exits. They always run from argument arrays.

use crate::process::{self, OutputMode, ProcessError, ProcessRequest};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

const LIST_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Larger clipboard images are refused rather than buffered.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_TYPE: &str = "image/png";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Wayland,
    X11,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// Neither `WAYLAND_DISPLAY` nor `DISPLAY` is set.
    NoDisplay,
    /// The session type is known but its clipboard tool is not installed.
    MissingTool {
        backend: Backend,
    },
    TooLarge,
    /// The clipboard offered an image this app cannot convert to PNG.
    UnsupportedImage(String),
    Tool(String),
}

impl ClipboardError {
    pub fn describe(&self) -> String {
        match self {
            Self::NoDisplay => {
                "No graphical session was found, so the clipboard is unavailable.".into()
            }
            Self::MissingTool {
                backend: Backend::Wayland,
            } => "Clipboard support needs wl-clipboard. Install it with \
                  \"sudo apt install wl-clipboard\"."
                .into(),
            Self::MissingTool {
                backend: Backend::X11,
            } => {
                "Clipboard support needs xclip. Install it with \"sudo apt install xclip\".".into()
            }
            Self::TooLarge => "The clipboard image is larger than 64 MB.".into(),
            Self::UnsupportedImage(kind) => {
                format!("The clipboard holds a {kind} image, which could not be converted to PNG.")
            }
            Self::Tool(message) => message.clone(),
        }
    }
}

pub trait ClipboardAccess {
    /// Returns the clipboard image as PNG bytes, or `None` without an image.
    fn read_png(&self) -> Result<Option<Vec<u8>>, ClipboardError>;
    fn set_text(&self, text: &str) -> Result<(), ClipboardError>;
}

/// Converts a non-PNG clipboard image to PNG. Supplied by the desktop shell,
/// which owns the image decoders.
pub type ImageConverter = fn(&[u8]) -> Option<Vec<u8>>;

#[derive(Debug, Clone)]
pub struct SystemClipboard {
    backend: Backend,
    paste: PathBuf,
    copy: PathBuf,
    convert: Option<ImageConverter>,
}

impl SystemClipboard {
    /// Chooses the session's clipboard tool. Wayland wins when both displays
    /// exist; an XWayland-only setup falls back to `xclip`.
    pub fn detect(
        env: impl Fn(&str) -> Option<OsString>,
        convert: Option<ImageConverter>,
    ) -> Result<Self, ClipboardError> {
        let set = |name: &str| env(name).filter(|value| !value.is_empty());
        let path = env("PATH");
        let find = |name: &str| process::find_on_path(name, path.as_deref());
        let wayland = set("WAYLAND_DISPLAY");
        let x11 = set("DISPLAY");

        if wayland.is_some()
            && let (Some(paste), Some(copy)) = (find("wl-paste"), find("wl-copy"))
        {
            return Ok(Self {
                backend: Backend::Wayland,
                paste,
                copy,
                convert,
            });
        }
        if x11.is_some()
            && let Some(xclip) = find("xclip")
        {
            return Ok(Self {
                backend: Backend::X11,
                paste: xclip.clone(),
                copy: xclip,
                convert,
            });
        }
        match (wayland, x11) {
            (Some(_), _) => Err(ClipboardError::MissingTool {
                backend: Backend::Wayland,
            }),
            (None, Some(_)) => Err(ClipboardError::MissingTool {
                backend: Backend::X11,
            }),
            (None, None) => Err(ClipboardError::NoDisplay),
        }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    fn request(&self, program: &PathBuf, arguments: &[&str], timeout: Duration) -> ProcessRequest {
        ProcessRequest::new(program, arguments, timeout)
    }

    fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
        let arguments: &[&str] = match self.backend {
            Backend::Wayland => &["--list-types"],
            Backend::X11 => &["-selection", "clipboard", "-target", "TARGETS", "-out"],
        };
        let output = process::run(&self.request(&self.paste, arguments, LIST_TIMEOUT))
            .map_err(tool_error)?;
        // An empty or unowned clipboard is reported as a failure by both tools.
        if !output.status.success() {
            return Ok(Vec::new());
        }
        Ok(parse_types(&String::from_utf8_lossy(&output.stdout)))
    }

    fn read_type(&self, mime: &str) -> Result<Vec<u8>, ClipboardError> {
        let arguments: Vec<&str> = match self.backend {
            Backend::Wayland => vec!["--no-newline", "--type", mime],
            Backend::X11 => vec!["-selection", "clipboard", "-target", mime, "-out"],
        };
        let mut request = self.request(&self.paste, &arguments, READ_TIMEOUT);
        request.max_output_bytes = MAX_IMAGE_BYTES;
        let output = process::run(&request).map_err(tool_error)?;
        if output.stdout_truncated {
            return Err(ClipboardError::TooLarge);
        }
        if !output.status.success() {
            return Err(ClipboardError::Tool(format!(
                "The clipboard image could not be read: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(output.stdout)
    }
}

impl ClipboardAccess for SystemClipboard {
    fn read_png(&self) -> Result<Option<Vec<u8>>, ClipboardError> {
        let types = self.list_types()?;
        let Some(mime) = preferred_image_type(&types) else {
            return Ok(None);
        };
        let bytes = self.read_type(mime)?;
        if mime == PNG_TYPE {
            return if is_png(&bytes) {
                Ok(Some(bytes))
            } else {
                Err(ClipboardError::UnsupportedImage("malformed PNG".into()))
            };
        }
        self.convert
            .and_then(|convert| convert(&bytes))
            .filter(|png| is_png(png))
            .map(Some)
            .ok_or_else(|| ClipboardError::UnsupportedImage(mime.to_owned()))
    }

    fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
        let arguments: &[&str] = match self.backend {
            Backend::Wayland => &["--type", "text/plain;charset=utf-8"],
            Backend::X11 => &["-selection", "clipboard", "-in"],
        };
        let mut request = self.request(&self.copy, arguments, WRITE_TIMEOUT);
        request.stdin = Some(text.as_bytes().to_vec());
        request.output = OutputMode::Discard;
        let output = process::run(&request).map_err(tool_error)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(ClipboardError::Tool(
                "The remote path could not be placed on the clipboard.".into(),
            ))
        }
    }
}

fn tool_error(error: ProcessError) -> ClipboardError {
    ClipboardError::Tool(match error {
        ProcessError::Timeout => "The clipboard did not respond in time.".into(),
        ProcessError::Spawn(error) | ProcessError::Wait(error) => {
            format!("The clipboard tool could not run: {error}")
        }
    })
}

pub fn parse_types(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// PNG first; otherwise the first other `image/*` type the clipboard offers.
pub fn preferred_image_type(types: &[String]) -> Option<&str> {
    types
        .iter()
        .find(|mime| mime.as_str() == PNG_TYPE)
        .or_else(|| types.iter().find(|mime| mime.starts_with("image/")))
        .map(String::as_str)
}

pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_dir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("tempdir");
        for name in ["wl-paste", "wl-copy", "xclip"] {
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
    fn wayland_session_uses_wl_clipboard() {
        let tools = tool_dir();
        let path = tools.path().to_str().expect("utf-8");
        let clipboard = SystemClipboard::detect(
            env(&[
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":0"),
                ("PATH", path),
            ]),
            None,
        )
        .expect("detect");
        assert_eq!(clipboard.backend(), Backend::Wayland);
        assert!(clipboard.paste.ends_with("wl-paste"));
        assert!(clipboard.copy.ends_with("wl-copy"));
    }

    #[test]
    fn xwayland_without_wl_clipboard_falls_back_to_xclip() {
        let tools = tool_dir();
        std::fs::remove_file(tools.path().join("wl-paste")).expect("remove");
        let path = tools.path().to_str().expect("utf-8");
        let clipboard = SystemClipboard::detect(
            env(&[
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":0"),
                ("PATH", path),
            ]),
            None,
        )
        .expect("detect");
        assert_eq!(clipboard.backend(), Backend::X11);
    }

    #[test]
    fn missing_tools_name_the_package_to_install() {
        let empty = tempfile::tempdir().expect("tempdir");
        let path = empty.path().to_str().expect("utf-8");
        let wayland =
            SystemClipboard::detect(env(&[("WAYLAND_DISPLAY", "w"), ("PATH", path)]), None)
                .expect_err("no tools");
        assert!(wayland.describe().contains("wl-clipboard"));
        let x11 = SystemClipboard::detect(env(&[("DISPLAY", ":1"), ("PATH", path)]), None)
            .expect_err("no tools");
        assert!(x11.describe().contains("xclip"));
        assert_eq!(
            SystemClipboard::detect(env(&[("DISPLAY", ""), ("PATH", path)]), None)
                .expect_err("no display"),
            ClipboardError::NoDisplay
        );
    }

    #[test]
    fn prefers_png_then_any_image_type() {
        let types = parse_types("TARGETS\ntext/plain\nimage/jpeg\nimage/png\n");
        assert_eq!(preferred_image_type(&types), Some("image/png"));
        let types = parse_types("text/uri-list\r\nimage/bmp\r\n");
        assert_eq!(preferred_image_type(&types), Some("image/bmp"));
        assert_eq!(
            preferred_image_type(&parse_types("UTF8_STRING\nTEXT")),
            None
        );
    }

    #[test]
    fn recognizes_the_png_signature() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_png(b"GIF89a"));
    }
}
