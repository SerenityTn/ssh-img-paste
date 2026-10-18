namespace SSHImagePaste.Windows;

public static class TrayMenuModel
{
    public static IReadOnlyList<DesktopAction> PrimaryActions { get; } =
    [
        DesktopAction.OpenWindow,
        DesktopAction.UploadClipboard,
        DesktopAction.CaptureRegion,
        DesktopAction.CaptureFullScreen,
        DesktopAction.UploadedImages,
        DesktopAction.ManageProfiles,
        DesktopAction.Quit,
    ];

    /// <summary>Actions preceded by a separator in the tray menu.</summary>
    public static bool StartsGroup(DesktopAction action) =>
        action is DesktopAction.UploadClipboard or DesktopAction.UploadedImages or DesktopAction.Quit;

    /// <summary>Actions that need a usable destination.</summary>
    public static bool RequiresDestination(DesktopAction action) =>
        action is DesktopAction.UploadClipboard or DesktopAction.CaptureRegion
            or DesktopAction.CaptureFullScreen or DesktopAction.UploadedImages;

    public static string Label(DesktopAction action) => action switch
    {
        DesktopAction.OpenWindow => "Open SSH Image Paste",
        DesktopAction.UploadClipboard => "Upload Clipboard Image",
        DesktopAction.CaptureRegion => "Capture Region",
        DesktopAction.CaptureFullScreen => "Capture Full Screen",
        DesktopAction.UploadedImages => "Uploaded Images…",
        DesktopAction.ManageProfiles => "Manage Profiles…",
        DesktopAction.Quit => "Quit",
        _ => throw new ArgumentOutOfRangeException(nameof(action)),
    };

    public const string DestinationMenuLabel = "Destination";
}
