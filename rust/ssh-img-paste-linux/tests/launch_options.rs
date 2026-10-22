use ssh_img_paste_linux::{LaunchRequest, parse_launch};

#[test]
fn launch_options_map_to_requests() {
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
    assert_eq!(
        parse(&["--capture-region"]),
        Ok(LaunchRequest::CaptureRegion)
    );
    assert_eq!(
        parse(&["--capture-full"]),
        Ok(LaunchRequest::CaptureFullScreen)
    );
    assert_eq!(parse(&["--background"]), Ok(LaunchRequest::Background));
    assert_eq!(parse(&["--quit"]), Ok(LaunchRequest::Quit));
    assert_eq!(parse(&["--smoke-test"]), Ok(LaunchRequest::SmokeTest));
    assert_eq!(parse(&["-h"]), Ok(LaunchRequest::Help));
    assert!(parse(&["--upload"]).is_err());
    assert!(parse(&["--quit", "--smoke-test"]).is_err());
}
