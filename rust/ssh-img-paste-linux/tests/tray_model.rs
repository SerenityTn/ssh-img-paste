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
