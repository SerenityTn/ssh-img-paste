using Microsoft.Win32;

namespace SSHImagePaste.Windows.Platform;

/// <summary>
/// Start-at-sign-in for the unpackaged (zip) build through the per-user Run
/// key. MSIX installs declare a StartupTask instead, which Windows manages in
/// Settings > Apps > Startup.
/// </summary>
public static class StartupRegistration
{
    private const string RunKeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "SSH Image Paste";
    public const string BackgroundArgument = "--background";

    public static bool IsPackaged
    {
        get
        {
            var length = 0;
            var result = NativeMethods.GetCurrentPackageFullName(ref length, null);
            return result != NativeMethods.AppModelErrorNoPackage;
        }
    }

    public static bool CanManage => !IsPackaged && AppPaths.ExecutablePath is not null;

    public static string Command(string executablePath) => $"\"{executablePath}\" {BackgroundArgument}";

    public static bool IsEnabled()
    {
        if (!CanManage)
        {
            return false;
        }
        using var key = Registry.CurrentUser.OpenSubKey(RunKeyPath, writable: false);
        return key?.GetValue(ValueName) is string value
            && string.Equals(value, Command(AppPaths.ExecutablePath!), StringComparison.OrdinalIgnoreCase);
    }

    public static void SetEnabled(bool enabled)
    {
        if (!CanManage)
        {
            return;
        }
        using var key = Registry.CurrentUser.CreateSubKey(RunKeyPath, writable: true);
        if (enabled)
        {
            key.SetValue(ValueName, Command(AppPaths.ExecutablePath!), RegistryValueKind.String);
        }
        else if (key.GetValue(ValueName) is not null)
        {
            key.DeleteValue(ValueName, throwOnMissingValue: false);
        }
    }

    /// <summary>Points an existing registration at this executable after an update or move.</summary>
    public static void RepairIfMoved()
    {
        if (!CanManage)
        {
            return;
        }
        using var key = Registry.CurrentUser.OpenSubKey(RunKeyPath, writable: false);
        if (key?.GetValue(ValueName) is string value
            && !string.Equals(value, Command(AppPaths.ExecutablePath!), StringComparison.OrdinalIgnoreCase))
        {
            SetEnabled(true);
        }
    }
}
