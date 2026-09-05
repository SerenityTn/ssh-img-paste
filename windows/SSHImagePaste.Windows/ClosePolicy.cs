namespace SSHImagePaste.Windows;

public enum CloseDecision
{
    HideToTray,
    ExitApplication,
}

public static class ClosePolicy
{
    public static CloseDecision Decide(bool trayAvailable) =>
        trayAvailable ? CloseDecision.HideToTray : CloseDecision.ExitApplication;
}
