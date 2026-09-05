namespace SSHImagePaste.Windows;

public static class TrayMenuModel
{
    public static IReadOnlyList<DesktopAction> PrimaryActions { get; } =
    [
        DesktopAction.OpenWindow,
        DesktopAction.UploadClipboard,
        DesktopAction.CaptureRegion,
        DesktopAction.CaptureFullScreen,
        DesktopAction.ManageProfiles,
        DesktopAction.Quit,
    ];

    public static string Label(DesktopAction action) => action switch
    {
        DesktopAction.OpenWindow => "Open SSH Image Paste",
        DesktopAction.UploadClipboard => "Upload Clipboard Image",
        DesktopAction.CaptureRegion => "Capture Region",
        DesktopAction.CaptureFullScreen => "Capture Full Screen",
        DesktopAction.ManageProfiles => "Manage Profiles…",
        DesktopAction.Quit => "Quit",
        _ => throw new ArgumentOutOfRangeException(nameof(action)),
    };
}
