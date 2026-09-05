using SSHImagePaste.Windows;

var failures = new List<string>();

void Check(bool condition, string message)
{
    if (!condition)
    {
        failures.Add(message);
    }
}

var expectedActions = new[]
{
    DesktopAction.OpenWindow,
    DesktopAction.UploadClipboard,
    DesktopAction.CaptureRegion,
    DesktopAction.CaptureFullScreen,
    DesktopAction.ManageProfiles,
    DesktopAction.Quit,
};
Check(TrayMenuModel.PrimaryActions.SequenceEqual(expectedActions), "tray action order differs from desktop parity contract");
Check(TrayInteraction.ActionForSingleLeftClick() is null, "single-clicking the tray icon must never upload or perform another side effect");
Check(TrayInteraction.ActionForDoubleClick() == DesktopAction.OpenWindow, "double-clicking the tray icon must only open the window");
Check(ClosePolicy.Decide(trayAvailable: true) == CloseDecision.HideToTray, "a usable tray should keep the application available after close");
Check(ClosePolicy.Decide(trayAvailable: false) == CloseDecision.ExitApplication, "without a usable tray, close must not strand an invisible process");
Check(TrayMenuModel.Label(DesktopAction.ManageProfiles) == "Manage Profiles…", "profile workflow labels should consistently indicate another workflow");

var hostileArgument = "profile name; $(touch should-not-run)";
var startInfo = CliInvocation.Create("C:\\Program Files\\SSH Image Paste\\ssh-img-paste.exe", new[]
{
    "upload",
    "--profile",
    hostileArgument,
});
Check(!startInfo.UseShellExecute, "CLI invocation must disable shell execution");
Check(startInfo.RedirectStandardOutput && startInfo.RedirectStandardError, "CLI output must be captured");
Check(startInfo.ArgumentList.Count == 3, "CLI arguments must remain separate");
Check(startInfo.ArgumentList[2] == hostileArgument, "CLI argument was altered or interpolated");
Check(startInfo.Arguments.Length == 0, "CLI invocation must not construct an Arguments command string");

if (failures.Count > 0)
{
    Console.Error.WriteLine(string.Join(Environment.NewLine, failures));
    return 1;
}

Console.WriteLine("PASS: Windows desktop shell contracts");
return 0;
