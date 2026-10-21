#![allow(clippy::result_large_err)] // CliError mirrors the protocol error object; it only travels on failure.

pub mod cli;
pub mod clipboard;
pub mod messages;
pub mod process;
pub mod staging;
pub mod upload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    UploadClipboard,
    CaptureRegion,
    CaptureFullScreen,
    ManageProfiles,
    Quit,
}

pub fn tray_actions() -> Vec<TrayAction> {
    vec![
        TrayAction::UploadClipboard,
        TrayAction::CaptureRegion,
        TrayAction::CaptureFullScreen,
        TrayAction::ManageProfiles,
        TrayAction::Quit,
    ]
}

pub const USAGE: &str = "\
Usage: ssh-img-paste-linux [OPTION]

Without an option, opens SSH Image Paste (or shows the running instance).

  --upload-clipboard  Upload the clipboard image and copy its remote path.
                      Bind this to a desktop keyboard shortcut; it reuses
                      the running instance when there is one.
  --quit              Quit the running instance.
  --smoke-test        Start, then quit after a moment (CI).
  --help              Show this help.
";

/// What one invocation asks the (possibly already running) app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchRequest {
    Open,
    UploadClipboard,
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
            "--quit" => Ok(LaunchRequest::Quit),
            "--smoke-test" => Ok(LaunchRequest::SmokeTest),
            "--help" | "-h" => Ok(LaunchRequest::Help),
            other => Err(format!("Unknown option: {other}")),
        },
        _ => Err("Pass at most one option.".into()),
    }
}
