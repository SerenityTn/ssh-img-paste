#![allow(clippy::result_large_err)] // CliError mirrors the protocol error object; it only travels on failure.

pub mod capture;
pub mod cli;
pub mod clipboard;
pub mod desktop;
pub mod messages;
pub mod process;
pub mod profile_rules;
pub mod staging;
pub mod upload;

pub const USAGE: &str = "\
Usage: ssh-img-paste-linux [OPTION]

Without an option, opens SSH Image Paste (or shows the running instance).

  --upload-clipboard  Upload the clipboard image (or take a screenshot when
                      it holds none) and copy the remote path. Bind this to
                      a desktop keyboard shortcut; it reuses the running
                      instance when there is one.
  --capture-region    Capture a region, upload it, and copy the remote path.
  --capture-full      Capture the full screen, upload it, and copy the path.
  --background        Start in the tray without opening the window.
  --quit              Quit the running instance.
  --smoke-test        Start, then quit after a moment (CI).
  --help              Show this help.
";

/// What one invocation asks the (possibly already running) app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchRequest {
    Open,
    Background,
    UploadClipboard,
    CaptureRegion,
    CaptureFullScreen,
    Quit,
    SmokeTest,
    Help,
}

/// Parses the arguments after the program name. At most one option.
pub fn parse_launch(arguments: &[String]) -> Result<LaunchRequest, String> {
    match arguments {
        [] => Ok(LaunchRequest::Open),
        [option] => match option.as_str() {
            "--upload-clipboard" => Ok(LaunchRequest::UploadClipboard),
            "--capture-region" => Ok(LaunchRequest::CaptureRegion),
            "--capture-full" => Ok(LaunchRequest::CaptureFullScreen),
            "--background" => Ok(LaunchRequest::Background),
            "--quit" => Ok(LaunchRequest::Quit),
            "--smoke-test" => Ok(LaunchRequest::SmokeTest),
            "--help" | "-h" => Ok(LaunchRequest::Help),
            other => Err(format!("Unknown option: {other}")),
        },
        _ => Err("Pass at most one option.".into()),
    }
}
