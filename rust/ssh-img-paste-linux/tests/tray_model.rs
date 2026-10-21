use ssh_img_paste_linux::{TrayAction, tray_actions};

#[test]
fn tray_model_lists_the_planned_primary_actions() {
    assert_eq!(
        tray_actions(),
        vec![
            TrayAction::UploadClipboard,
            TrayAction::CaptureRegion,
            TrayAction::CaptureFullScreen,
            TrayAction::ManageProfiles,
            TrayAction::Quit,
        ]
    );
}

#[test]
fn launch_options_map_to_requests() {
    use ssh_img_paste_linux::{LaunchRequest, parse_launch};
    let parse = |arguments: &[&str]| {
        parse_launch(
            &arguments
                .iter()
                .map(|argument| argument.to_string())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(parse(&[]), Ok(LaunchRequest::Open));
    assert_eq!(
        parse(&["--upload-clipboard"]),
        Ok(LaunchRequest::UploadClipboard)
    );
    assert_eq!(parse(&["--quit"]), Ok(LaunchRequest::Quit));
    assert_eq!(parse(&["--smoke-test"]), Ok(LaunchRequest::SmokeTest));
    assert_eq!(parse(&["-h"]), Ok(LaunchRequest::Help));
    assert!(parse(&["--upload"]).is_err());
    assert!(parse(&["--quit", "--smoke-test"]).is_err());
}
