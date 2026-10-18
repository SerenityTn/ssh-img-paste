namespace SSHImagePaste.Windows;

public static class TrayInteraction
{
    /// <summary>Matches the macOS menu-bar icon: one click uploads.</summary>
    public static DesktopAction ActionForSingleLeftClick() => DesktopAction.UploadClipboard;

    public static DesktopAction ActionForDoubleClick() => DesktopAction.OpenWindow;
}
