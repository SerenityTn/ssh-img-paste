using SSHImagePaste.Windows.Imaging;

// Usage: dotnet run --project windows/tools/AssetGenerator -- <windows directory> [preview directory]
var windowsDirectory = Path.GetFullPath(args.Length > 0 ? args[0] : Path.Combine("..", ".."));
var appAssets = Path.Combine(windowsDirectory, "SSHImagePaste.Windows", "Assets");
var msixAssets = Path.Combine(windowsDirectory, "packaging", "msix", "Assets");
Directory.CreateDirectory(appAssets);
Directory.CreateDirectory(msixAssets);

var thread = new Thread(() =>
{
    var icon = IconFactory.CreateAppIcon();
    File.WriteAllBytes(Path.Combine(appAssets, "AppIcon.ico"), IconFactory.CreateIco(icon, [16, 20, 24, 32, 40, 48, 64, 256]));

    var logos = new (string Name, int Size)[]
    {
        ("StoreLogo.png", 50),
        ("Square44x44Logo.png", 44),
        ("Square44x44Logo.targetsize-16.png", 16),
        ("Square44x44Logo.targetsize-24.png", 24),
        ("Square44x44Logo.targetsize-32.png", 32),
        ("Square44x44Logo.targetsize-48.png", 48),
        ("Square44x44Logo.targetsize-256.png", 256),
        ("Square44x44Logo.targetsize-16_altform-unplated.png", 16),
        ("Square44x44Logo.targetsize-24_altform-unplated.png", 24),
        ("Square44x44Logo.targetsize-32_altform-unplated.png", 32),
        ("Square44x44Logo.targetsize-48_altform-unplated.png", 48),
        ("Square44x44Logo.targetsize-256_altform-unplated.png", 256),
        ("Square150x150Logo.png", 150),
        ("Square310x310Logo.png", 310),
    };
    foreach (var (name, size) in logos)
    {
        File.WriteAllBytes(Path.Combine(msixAssets, name), IconFactory.RenderPng(icon, size));
    }
    if (args.Length > 1)
    {
        Directory.CreateDirectory(args[1]);
        File.WriteAllBytes(Path.Combine(args[1], "app-512.png"), IconFactory.RenderPng(icon, 512));
        foreach (var state in Enum.GetValues<TrayState>())
        {
            foreach (var size in new[] { 16, 32, 64 })
            {
                File.WriteAllBytes(
                    Path.Combine(args[1], $"tray-{state.ToString().ToLowerInvariant()}-{size}.png"),
                    IconFactory.RenderPng(IconFactory.CreateTrayIcon(state), size));
            }
        }
    }
    Console.WriteLine($"Wrote icons to {appAssets} and {msixAssets}");
});
thread.SetApartmentState(ApartmentState.STA);
thread.Start();
thread.Join();
