namespace SSHImagePaste.Windows;

public static class TrayInteraction
{
    public static DesktopAction? ActionForSingleLeftClick() => null;

    public static DesktopAction ActionForDoubleClick() => DesktopAction.OpenWindow;
}
