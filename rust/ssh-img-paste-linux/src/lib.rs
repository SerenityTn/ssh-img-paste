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
