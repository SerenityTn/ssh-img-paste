using System.Text.Json;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using SSHImagePaste.Windows;
using SSHImagePaste.Windows.Capture;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Clipboard;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;
using SSHImagePaste.Windows.Profiles;

namespace SSHImagePaste.Windows.ContractTests;

/// <summary>
/// Dependency-free contract suite for the Windows shell. When the Rust CLI
/// is available (built next to the app, or via SSH_IMG_PASTE_CLI), the
/// service layer is also exercised against the real binary, and the fake
/// OpenSSH example (SSH_IMG_PASTE_FAKE_OPENSSH) covers remote commands.
/// </summary>
public static class Program
{
    private static readonly List<string> Failures = [];
    private static int checks;

    private static void Check(bool condition, string message)
    {
        checks++;
        if (!condition)
        {
            Failures.Add(message);
        }
    }

    private static void Section(string name, Action body)
    {
        try
        {
            body();
        }
        catch (Exception exception)
        {
            Failures.Add($"{name}: unexpected {exception.GetType().Name}: {exception.Message}");
        }
    }

    [STAThread]
    public static int Main()
    {
        Section("tray", TrayContracts);
        Section("cli invocation", InvocationContracts);
        Section("protocol parsing", ProtocolContracts);
        Section("profile rules", ProfileRuleContracts);
        Section("clipboard restore policy", ClipboardPolicyContracts);
        Section("selection geometry", SelectionContracts);
        Section("error messages", ErrorMessageContracts);
        Section("imaging", ImagingContracts);
        Section("startup", StartupContracts);
        Section("real cli", RealCliContracts);

        if (Failures.Count > 0)
        {
            Console.Error.WriteLine($"FAIL: {Failures.Count} of {checks} checks failed");
            Console.Error.WriteLine(string.Join(Environment.NewLine, Failures.Select(failure => $"  - {failure}")));
            return 1;
        }
        Console.WriteLine($"PASS: Windows desktop shell contracts ({checks} checks)");
        return 0;
    }

    private static void TrayContracts()
    {
        DesktopAction[] expected =
        [
            DesktopAction.OpenWindow,
            DesktopAction.UploadClipboard,
            DesktopAction.CaptureRegion,
            DesktopAction.CaptureFullScreen,
            DesktopAction.UploadedImages,
            DesktopAction.ManageProfiles,
            DesktopAction.Quit,
        ];
        Check(TrayMenuModel.PrimaryActions.SequenceEqual(expected), "tray action order differs from desktop parity contract");
        Check(TrayInteraction.ActionForSingleLeftClick() is null, "single-clicking the tray icon must never upload or perform another side effect");
        Check(TrayInteraction.ActionForDoubleClick() == DesktopAction.OpenWindow, "double-clicking the tray icon must only open the window");
        Check(ClosePolicy.Decide(trayAvailable: true) == CloseDecision.HideToTray, "a usable tray should keep the application available after close");
        Check(ClosePolicy.Decide(trayAvailable: false) == CloseDecision.ExitApplication, "without a usable tray, close must not strand an invisible process");
        Check(TrayMenuModel.Label(DesktopAction.ManageProfiles) == "Manage Profiles…", "profile workflow labels should consistently indicate another workflow");
        Check(TrayMenuModel.Label(DesktopAction.UploadedImages).EndsWith('…'), "uploaded images opens another window and needs an ellipsis");
        Check(TrayMenuModel.RequiresDestination(DesktopAction.UploadClipboard) && !TrayMenuModel.RequiresDestination(DesktopAction.ManageProfiles),
            "uploads need a destination; managing profiles must always be reachable");
        Check(!TrayMenuModel.RequiresDestination(DesktopAction.Quit) && !TrayMenuModel.RequiresDestination(DesktopAction.OpenWindow),
            "quit and open must never be disabled");
    }

    private static void InvocationContracts()
    {
        var hostile = "profile name; $(touch should-not-run) & calc.exe | \"quoted\"";
        var startInfo = CliInvocation.Create(@"C:\Program Files\SSH Image Paste\ssh-img-paste.exe", ["upload", "--profile", hostile]);
        Check(!startInfo.UseShellExecute, "CLI invocation must disable shell execution");
        Check(startInfo.CreateNoWindow, "CLI invocation must not flash a console window");
        Check(startInfo.RedirectStandardOutput && startInfo.RedirectStandardError && startInfo.RedirectStandardInput, "CLI streams must be captured and stdin closed");
        Check(startInfo.ArgumentList.Count == 3, "CLI arguments must remain separate");
        Check(startInfo.ArgumentList[2] == hostile, "CLI argument was altered or interpolated");
        Check(startInfo.Arguments.Length == 0, "CLI invocation must not construct an Arguments command string");

        var fields = new ProfileFields("Work; rm", "work-ssh", "/home/me", "img", ShotMode.Full, 30).ToArguments().ToList();
        Check(fields.SequenceEqual(["--label", "Work; rm", "--host", "work-ssh", "--remote-home", "/home/me", "--remote-dir", "img", "--shot-mode", "full", "--restore-seconds", "30"]),
            "profile fields must map to separate flag/value arguments");
        Check(!new ProfileFields(Label: "x").ToArguments().Contains("--host"), "unset fields must not be sent");
    }

    private static CliError? ErrorOf(string stdout, int exitCode)
    {
        try
        {
            CliClient.ParseDocument(stdout, exitCode, ["test"]);
            return null;
        }
        catch (CliException exception)
        {
            return exception.Error;
        }
    }

    private static void ProtocolContracts()
    {
        var result = CliClient.ParseDocument("{\"protocol_version\":1,\"ok\":true,\"result\":{\"type\":\"x\",\"n\":3}}\n", 0, ["test"]);
        Check(result.GetProperty("n").GetInt32() == 3, "a success document must return its result");

        var error = ErrorOf("{\"protocol_version\":1,\"ok\":false,\"error\":{\"code\":\"upload_failed\",\"message\":\"m\",\"step\":\"upload\",\"reason\":\"exit\",\"exit_code\":255,\"diagnostic\":\"Permission denied\",\"field\":\"host\"}}", 3);
        Check(error is { Code: "upload_failed", Step: "upload", Reason: "exit", ExitCode: 255, Diagnostic: "Permission denied", Field: "host" },
            "structured error fields must be preserved");
        Check(ErrorOf("{\"protocol_version\":2,\"ok\":true,\"result\":{}}", 0)?.Code == "cli_protocol", "an unknown protocol version must be rejected");
        Check(ErrorOf("not json", 0)?.Code == "cli_protocol", "non-JSON output must be rejected");
        Check(ErrorOf("{\"protocol_version\":1,\"ok\":true,\"result\":{}}", 3)?.Code == "cli_protocol", "ok with a failing exit code is inconsistent");
        Check(ErrorOf("[1,2]", 0)?.Code == "cli_protocol", "non-object documents must be rejected");

        using var details = JsonDocument.Parse("""
            {"id":"broken","usable":false,"active":false,"problem":{"code":"dynamic_syntax","key":"SSH_HOST"}}
            """);
        var broken = Json.Profile(details.RootElement);
        Check(!broken.Usable && broken.Problem is { Code: "dynamic_syntax", Key: "SSH_HOST" } && broken.DisplayName == "broken",
            "unusable profiles must keep their problem details");
        using var usable = JsonDocument.Parse("""
            {"id":"work","usable":true,"active":true,"label":"Work","host":"h","remote_home":"/home/me/","remote_dir":"img/","shot_mode":"full","restore_seconds":"0","editable":true}
            """);
        var work = Json.Profile(usable.RootElement);
        Check(work is { ShotMode: ShotMode.Full, RestoreSeconds: 0, Editable: true } && work.RemoteRoot == "/home/me/img",
            "profile details must parse shot mode, restore delay, and the remote root");
    }

    private static void ProfileRuleContracts()
    {
        Check(ProfileRules.SuggestId("Work SSH Host", []) == "work-ssh-host", "IDs should be derived from the display name");
        Check(ProfileRules.SuggestId("Work", ["work", "work-2"]) == "work-3", "suggested IDs must not collide (case-insensitively)");
        Check(ProfileRules.SuggestId("Café — Prod!", []) == "cafe-prod", "accents and punctuation should be simplified");
        Check(ProfileRules.SuggestId("!!!", []) == "destination", "an empty slug falls back to a generic ID");
        Check(ProfileRules.IsValidId(ProfileRules.SuggestId(new string('x', 200), [])), "long names must still yield a valid ID");
        Check(ProfileRules.ValidateHost("host:22") is not null, "ports belong in SSH config, not the host field");
        Check(ProfileRules.ValidateRemoteHome("C:/Users/me") is not null, "a Windows path is never a valid remote home");
        Check(ProfileRules.ValidateRestoreSeconds("86401") is not null && ProfileRules.ValidateRestoreSeconds("0") is null, "restore delay bounds");
    }

    private static void ClipboardPolicyContracts()
    {
        var policy = new ClipboardRestorePolicy();
        var first = policy.Begin("/home/me/a.png", 10);
        Check(policy.ShouldRestore(first, "/home/me/a.png", 10), "an untouched claim should restore");
        Check(!policy.ShouldRestore(first, "/home/me/a.png", 11), "a newer clipboard sequence means another app wrote it");
        Check(!policy.ShouldRestore(first, "something else", 10), "different text must never be overwritten");
        var second = policy.Begin("/home/me/b.png", 12);
        Check(!policy.ShouldRestore(first, "/home/me/a.png", 10), "a newer upload supersedes the older timer");
        Check(policy.ShouldRestore(second, "/home/me/b.png", 12), "the newest claim stays restorable");
        policy.Complete(first);
        Check(policy.IsActive(second), "completing a stale claim must not clear the active one");
        policy.Complete(second);
        Check(!policy.ShouldRestore(second, "/home/me/b.png", 12), "a completed claim never restores twice");
        var unknown = policy.Begin("/x.png", null);
        Check(!policy.ShouldRestore(unknown, "/x.png", null), "without an ownership marker, restoration is refused");
    }

    private static void SelectionContracts()
    {
        var rectangle = SelectionGeometry.Normalize(new System.Drawing.Point(100, 50), new System.Drawing.Point(20, 10));
        Check(rectangle == new System.Drawing.Rectangle(20, 10, 81, 41), "dragging up-left must normalize to a positive rectangle");
        Check(!SelectionGeometry.IsUsable(new System.Drawing.Rectangle(0, 0, 2, 50)), "accidental clicks must not capture");
        var virtualScreen = new System.Drawing.Rectangle(-1920, 0, 3840, 1080);
        Check(SelectionGeometry.ToCapture(new System.Drawing.Rectangle(-100, 10, 200, 20), virtualScreen) == new System.Drawing.Rectangle(1820, 10, 200, 20),
            "selections on monitors left of the primary map into capture coordinates");
        Check(SelectionGeometry.ToCapture(new System.Drawing.Rectangle(1900, 1070, 100, 100), virtualScreen) == new System.Drawing.Rectangle(3820, 1070, 20, 10),
            "selections are clipped to the virtual screen");
    }

    private static void ErrorMessageContracts()
    {
        string Describe(string diagnostic, string? reason = "exit") =>
            ErrorMessages.Describe(new CliError("upload_failed", "The remote upload did not complete.", Reason: reason, Diagnostic: diagnostic));
        Check(Describe("Host key verification failed.").Contains("accept the key"), "unknown host keys need a first interactive ssh");
        Check(Describe("me@h: Permission denied (publickey).").Contains("ssh-agent"), "key rejection should point to the agent");
        Check(Describe("ssh: Could not resolve hostname x").Contains("resolved"), "DNS failures should mention the alias");
        Check(Describe("", "spawn").Contains("OpenSSH Client"), "a missing ssh.exe should point to the Windows optional feature");
        Check(Describe("", "timeout").Contains("did not respond"), "timeouts should be explained");
        Check(ErrorMessages.Describe(new CliError("invalid_field", "x", Field: "remote_home")).StartsWith("Remote home"), "field errors name the field");
    }

    private static void ImagingContracts()
    {
        // Clipboard DIBs whose padding alpha is zero must not upload as a transparent image.
        var pixels = new byte[4 * 4 * 4];
        for (var index = 0; index < pixels.Length; index += 4)
        {
            pixels[index] = 200;
            pixels[index + 1] = 100;
            pixels[index + 2] = 50;
        }
        var transparent = BitmapSource.Create(4, 4, 96, 96, PixelFormats.Bgra32, null, pixels, 16);
        var png = PngEncoding.Encode(transparent);
        Check(PngEncoding.IsPng(png), "encoding must produce a PNG signature");
        var decoded = BitmapDecoder.Create(new MemoryStream(png), BitmapCreateOptions.None, BitmapCacheOption.OnLoad).Frames[0];
        var converted = new FormatConvertedBitmap(decoded, PixelFormats.Bgra32, null, 0);
        var output = new byte[16 * 4];
        converted.CopyPixels(output, 16, 0);
        Check(output[3] == 255 && output[2] == 50, "zero-alpha clipboard bitmaps must become opaque");

        for (var index = 3; index < pixels.Length; index += 4)
        {
            pixels[index] = (byte)(index % 8 == 3 ? 128 : 255);
        }
        var translucent = PngEncoding.Encode(BitmapSource.Create(4, 4, 96, 96, PixelFormats.Bgra32, null, pixels, 16));
        var translucentFrame = new FormatConvertedBitmap(
            BitmapDecoder.Create(new MemoryStream(translucent), BitmapCreateOptions.None, BitmapCacheOption.OnLoad).Frames[0],
            PixelFormats.Bgra32, null, 0);
        translucentFrame.CopyPixels(output, 16, 0);
        Check(output[3] == 128, "real transparency must be preserved");

        using var gdi = new System.Drawing.Bitmap(7, 5, System.Drawing.Imaging.PixelFormat.Format32bppRgb);
        Check(PngEncoding.Dimensions(PngEncoding.Encode(gdi)) == (7, 5), "GDI captures must keep their pixel size");

        var ico = IconFactory.CreateIco(IconFactory.CreateTrayIcon(TrayState.Success), [16, 32, 256]);
        using var icon = new System.Drawing.Icon(new MemoryStream(ico), 32, 32);
        Check(icon.Width == 32, "generated icons must load through System.Drawing");
        using var tray = IconFactory.CreateTrayIconHandle(TrayState.Error, 24);
        Check(tray.Width == 24, "tray icons must be generated at the requested size");
    }

    private static void StartupContracts()
    {
        Check(StartupRegistration.Command(@"C:\Users\Me\AppData\Local\Programs\SSH Image Paste\SSHImagePaste.exe")
                == "\"C:\\Users\\Me\\AppData\\Local\\Programs\\SSH Image Paste\\SSHImagePaste.exe\" --background",
            "the Run command must quote the path and start in the background");
    }

    private static string? FindCli()
    {
        var configured = Environment.GetEnvironmentVariable("SSH_IMG_PASTE_CLI");
        if (!string.IsNullOrEmpty(configured))
        {
            return configured;
        }
        var bundled = Path.Combine(AppContext.BaseDirectory, AppPaths.CliFileName);
        return File.Exists(bundled) ? bundled : null;
    }

    private static void RealCliContracts()
    {
        if (FindCli() is not { } cli)
        {
            Console.WriteLine("SKIP: real CLI contracts (set SSH_IMG_PASTE_CLI or build the Rust CLI first)");
            return;
        }
        var root = Path.Combine(Path.GetTempPath(), $"ssh-img-paste-shell-contract-{Environment.ProcessId}");
        if (Directory.Exists(root))
        {
            Directory.Delete(root, recursive: true);
        }
        try
        {
            var service = new SshImagePasteService(new CliClient(cli, root));
            T Run<T>(Func<Task<T>> action) => action().GetAwaiter().GetResult();
            void RunVoid(Func<Task> action) => action().GetAwaiter().GetResult();
            CliError? Fails(Func<Task> action)
            {
                try
                {
                    RunVoid(action);
                    return null;
                }
                catch (CliException exception)
                {
                    return exception.Error;
                }
            }

            var empty = Run(() => service.GetCatalogAsync());
            Check(!empty.Configured && empty.Profiles.Count == 0, "a fresh install has an empty, unconfigured catalog");

            var fields = new ProfileFields("Work \"west\" $HOME", "work-ssh", "/home/me", "img-uploads", ShotMode.Region, 45);
            Check(Run(() => service.CreateProfileAsync("work", fields)), "the first destination becomes active");
            Check(!Run(() => service.CreateProfileAsync("home", fields with { Label = "Home", Host = "me@home.example" })), "later destinations do not steal active");
            var catalog = Run(() => service.GetCatalogAsync());
            Check(catalog.ActiveProfileId == "work" && catalog.Profiles.Count == 2, "the catalog reflects created destinations");
            Check(catalog.Active is { Label: "Work \"west\" $HOME", RestoreSeconds: 45, ShotMode: ShotMode.Region, Editable: true },
                "labels with shell characters round-trip literally");

            RunVoid(() => service.UpdateProfileAsync("home", new ProfileFields(ShotMode: ShotMode.Full)));
            RunVoid(() => service.UseProfileAsync("home"));
            catalog = Run(() => service.GetCatalogAsync());
            Check(catalog.Active is { Id: "home", ShotMode: ShotMode.Full }, "update and use are applied");

            RunVoid(() => service.RenameProfileAsync("home", "house"));
            Check(Run(() => service.GetCatalogAsync()).ActiveProfileId == "house", "renaming the active destination keeps it active");
            Check(Fails(() => service.DeleteProfileAsync("house", null))?.Code == "switch_to_required", "deleting active needs a replacement");
            RunVoid(() => service.DeleteProfileAsync("house", "work"));
            Check(Fails(() => service.DeleteProfileAsync("work", null))?.Code == "last_usable_profile", "the last destination is protected");

            // Client-side rules must agree with the authoritative CLI.
            string[] hosts = ["work-ssh", "me@host.example", "-oProxyCommand=x", "host:22", "a b", "h;x", "ünicode", "h_1.example"];
            foreach (var host in hosts)
            {
                var cliAccepts = Fails(() => service.UpdateProfileAsync("work", new ProfileFields(Host: host))) is null;
                Check(cliAccepts == (ProfileRules.ValidateHost(host) is null), $"host rule mismatch for {host}");
            }
            string[] homes = ["/home/me", "/", "relative", "/home/me/../x", "/home//me", "/home/with space", "/home/$USER", "C:/Users/me"];
            foreach (var home in homes)
            {
                var cliAccepts = Fails(() => service.UpdateProfileAsync("work", new ProfileFields(RemoteHome: home))) is null;
                Check(cliAccepts == (ProfileRules.ValidateRemoteHome(home) is null), $"remote home rule mismatch for {home}");
            }
            string[] dirs = ["img", "a/b", "/abs", "-x", ".", "a/../b", "a b", "img-uploads/"];
            foreach (var dir in dirs)
            {
                var cliAccepts = Fails(() => service.UpdateProfileAsync("work", new ProfileFields(RemoteDirectory: dir))) is null;
                Check(cliAccepts == (ProfileRules.ValidateRemoteDirectory(dir) is null), $"upload folder rule mismatch for {dir}");
            }
            RunVoid(() => service.UpdateProfileAsync("work", new ProfileFields(Host: "work-ssh", RemoteHome: "/home/me", RemoteDirectory: "img-uploads")));

            var version = Run(() => service.GetVersionAsync());
            Check(version.Containment == "job_object", "Windows OpenSSH children must be contained by a Job Object");

            RemoteContracts(service, root);
        }
        finally
        {
            try
            {
                Directory.Delete(root, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    private static void RemoteContracts(SshImagePasteService service, string root)
    {
        var fake = Environment.GetEnvironmentVariable("SSH_IMG_PASTE_FAKE_OPENSSH");
        if (string.IsNullOrEmpty(fake) || !File.Exists(fake))
        {
            Console.WriteLine("SKIP: remote contracts (set SSH_IMG_PASTE_FAKE_OPENSSH to the fake_openssh example)");
            return;
        }
        var bin = Path.Combine(root, "openssh");
        Directory.CreateDirectory(bin);
        File.Copy(fake, Path.Combine(bin, "ssh.exe"));
        File.Copy(fake, Path.Combine(bin, "scp.exe"));
        var log = Path.Combine(root, "openssh.log");
        Environment.SetEnvironmentVariable("SSH_IMG_PASTE_OPENSSH_DIR", bin);
        Environment.SetEnvironmentVariable("FAKE_OPENSSH_LOG", log);
        try
        {
            var source = Path.Combine(root, "shot.png");
            var copy = Path.Combine(root, "transferred.png");
            using (var gdi = new System.Drawing.Bitmap(3, 2))
            {
                File.WriteAllBytes(source, PngEncoding.Encode(gdi));
            }
            Environment.SetEnvironmentVariable("FAKE_OPENSSH_UPLOAD_COPY", copy);
            var remotePath = service.UploadFileAsync(source, "work").GetAwaiter().GetResult();
            Check(remotePath.StartsWith("/home/me/img-uploads/ssh-img-", StringComparison.Ordinal) && remotePath.EndsWith(".png", StringComparison.Ordinal),
                "uploads return the remote path inside the configured folder");
            Check(File.ReadAllBytes(copy).SequenceEqual(File.ReadAllBytes(source)), "the exact PNG bytes are transferred");
            Environment.SetEnvironmentVariable("FAKE_OPENSSH_UPLOAD_COPY", null);

            Environment.SetEnvironmentVariable("FAKE_OPENSSH_STDOUT", "1700000100.5\t123\tb.png\n1700000000\t9\ta.png\n");
            var listing = service.ListUploadsAsync("work").GetAwaiter().GetResult();
            Check(listing.Uploads.Select(upload => upload.Name).SequenceEqual(["b.png", "a.png"]), "listings are newest first");
            Check(listing.Uploads[0].RemotePath == "/home/me/img-uploads/b.png" && listing.Uploads[0].SizeBytes == 123, "listing rows carry path and size");

            Environment.SetEnvironmentVariable("FAKE_OPENSSH_STDOUT", "/home/me\n");
            Check(service.TestProfileAsync("work").GetAwaiter().GetResult().HomeMatches, "connection test compares the detected home");
            Check(service.ProbeHostAsync("other@host").GetAwaiter().GetResult() == "/home/me", "probing an unsaved host returns its home");

            Environment.SetEnvironmentVariable("FAKE_OPENSSH_STDOUT", "2\n");
            Check(service.CleanUploadsAsync("work").GetAwaiter().GetResult() == 2, "clean reports the deleted count");

            Environment.SetEnvironmentVariable("FAKE_OPENSSH_STDOUT", null);
            Environment.SetEnvironmentVariable("FAKE_OPENSSH_FETCH_SOURCE", source);
            var local = service.FetchUploadAsync("work", "b.png").GetAwaiter().GetResult();
            Check(File.Exists(local) && Path.GetFileName(local) == "b.png", "fetch downloads into a local PNG path");
            Directory.Delete(Path.GetDirectoryName(local)!, recursive: true);
            Environment.SetEnvironmentVariable("FAKE_OPENSSH_FETCH_SOURCE", null);

            Environment.SetEnvironmentVariable("FAKE_OPENSSH_EXIT", "255");
            Environment.SetEnvironmentVariable("FAKE_OPENSSH_STDERR", "Host key verification failed.\r\n");
            try
            {
                service.TestProfileAsync("work").GetAwaiter().GetResult();
                Check(false, "a failed connection must raise");
            }
            catch (CliException exception)
            {
                Check(ErrorMessages.Describe(exception.Error).Contains("accept the key"), "real CLI diagnostics map to the host-key hint");
            }
        }
        finally
        {
            foreach (var name in new[] { "SSH_IMG_PASTE_OPENSSH_DIR", "FAKE_OPENSSH_LOG", "FAKE_OPENSSH_STDOUT", "FAKE_OPENSSH_EXIT", "FAKE_OPENSSH_STDERR", "FAKE_OPENSSH_FETCH_SOURCE", "FAKE_OPENSSH_UPLOAD_COPY" })
            {
                Environment.SetEnvironmentVariable(name, null);
            }
        }
    }
}
