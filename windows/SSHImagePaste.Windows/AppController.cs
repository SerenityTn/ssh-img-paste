using System.Windows;
using System.Windows.Threading;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Clipboard;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;
using SSHImagePaste.Windows.Tray;
using SSHImagePaste.Windows.Upload;

namespace SSHImagePaste.Windows;

/// <summary>What the main window and tray show about the latest activity.</summary>
public sealed record AppStatus(TrayState State, string Message, string? RemotePath = null);

/// <summary>Coordinates the tray, windows, uploads, and the profile catalog.</summary>
public sealed class AppController : IDisposable
{
    private readonly Dispatcher dispatcher;
    private readonly Action shutdown;
    private readonly FileSystemWatcher? watcher;
    private readonly DispatcherTimer catalogDebounce;
    private TrayController? tray;
    private MainWindow? mainWindow;
    private ProfilesWindow? profilesWindow;
    private UploadsWindow? uploadsWindow;
    private bool refreshing;
    private bool refreshQueued;

    public AppController(Dispatcher dispatcher, Action shutdown)
    {
        this.dispatcher = dispatcher;
        this.shutdown = shutdown;
        Cli = new CliClient(AppPaths.BundledCliPath, AppPaths.ConfigRootOverride);
        Service = new SshImagePasteService(Cli);
        Clipboard = new ClipboardService();
        Uploads = new UploadCoordinator(Service, Clipboard, HideWindowsForCapture);
        Uploads.Progress += (_, message) => dispatcher.BeginInvoke(() => Report(new AppStatus(TrayState.Busy, message)));

        catalogDebounce = new DispatcherTimer(TimeSpan.FromMilliseconds(400), DispatcherPriority.Background, (_, _) =>
        {
            catalogDebounce!.Stop();
            _ = RefreshCatalogAsync();
        }, dispatcher);
        catalogDebounce.Stop();
        watcher = CreateWatcher();
    }

    public CliClient Cli { get; }

    public SshImagePasteService Service { get; }

    public ClipboardService Clipboard { get; }

    public UploadCoordinator Uploads { get; }

    public ProfileCatalog? Catalog { get; private set; }

    public string? CatalogError { get; private set; }

    public AppStatus Status { get; private set; } = new(TrayState.Idle, "Ready.");

    public bool TrayAvailable => tray is not null;

    public event EventHandler? CatalogChanged;

    public event EventHandler? StatusChanged;

    public void Start(bool background)
    {
        StagingFiles.RemoveStale();
        StartupRegistration.RepairIfMoved();
        try
        {
            tray = new TrayController();
            tray.ActionRequested += (_, action) => Dispatch(action);
            tray.DestinationRequested += (_, id) => _ = UseDestinationAsync(id);
            tray.MenuOpening += (_, _) => _ = RefreshCatalogAsync();
        }
        catch (Exception exception)
        {
            Log.Error("Tray unavailable", exception);
            tray?.Dispose();
            tray = null;
        }

        mainWindow = new MainWindow(this);
        if (!background || tray is null)
        {
            ShowMain();
        }
        if (tray is null)
        {
            mainWindow.ReportTrayUnavailable();
        }
        _ = InitializeAsync(background);
    }

    private async Task InitializeAsync(bool background)
    {
        await RefreshCatalogAsync();
        if (Catalog is { } catalog && !catalog.Usable.Any() && CatalogError is null)
        {
            Report(new AppStatus(TrayState.Idle, "Add an SSH destination to get started."));
            if (!background)
            {
                ShowProfiles(startAdding: true);
            }
            else
            {
                tray?.Notify(AppPaths.ProductName, "Add an SSH destination to start uploading. Double-click the tray icon to open.", NotificationKind.Info);
            }
        }
    }

    public void Dispatch(DesktopAction action)
    {
        switch (action)
        {
            case DesktopAction.OpenWindow:
                ShowMain();
                break;
            case DesktopAction.UploadClipboard:
                _ = UploadAsync(UploadSource.Auto);
                break;
            case DesktopAction.CaptureRegion:
                _ = UploadAsync(UploadSource.Region);
                break;
            case DesktopAction.CaptureFullScreen:
                _ = UploadAsync(UploadSource.FullScreen);
                break;
            case DesktopAction.UploadedImages:
                ShowUploads();
                break;
            case DesktopAction.ManageProfiles:
                ShowProfiles();
                break;
            case DesktopAction.Quit:
                shutdown();
                break;
        }
    }

    public async Task RefreshCatalogAsync()
    {
        if (refreshing)
        {
            refreshQueued = true;
            return;
        }
        refreshing = true;
        try
        {
            do
            {
                refreshQueued = false;
                try
                {
                    Catalog = await Service.GetCatalogAsync();
                    CatalogError = null;
                }
                catch (CliException exception)
                {
                    CatalogError = ErrorMessages.Describe(exception.Error);
                    Log.Warn($"Catalog refresh failed: {exception.Error.Code}");
                }
                tray?.UpdateCatalog(Catalog);
                CatalogChanged?.Invoke(this, EventArgs.Empty);
            }
            while (refreshQueued);
        }
        finally
        {
            refreshing = false;
        }
    }

    public async Task UseDestinationAsync(string id)
    {
        try
        {
            await Service.UseProfileAsync(id);
            await RefreshCatalogAsync();
            var label = Catalog?.Active?.Label ?? id;
            Report(new AppStatus(TrayState.Idle, $"Uploads now go to {label}."));
        }
        catch (CliException exception)
        {
            ReportFailure(ErrorMessages.Describe(exception.Error));
        }
    }

    public async Task<UploadResult> UploadAsync(UploadSource source)
    {
        if (Uploads.IsBusy)
        {
            tray?.Notify(AppPaths.ProductName, "An upload is already in progress.", NotificationKind.Info);
            return new UploadResult(UploadResultKind.Busy, "An upload is already in progress.");
        }
        tray?.SetBusy("Uploading…");
        var result = await Uploads.RunAsync(source);
        switch (result.Kind)
        {
            case UploadResultKind.Uploaded:
                var restore = result.RestoreSeconds > 0
                    ? $"The previous clipboard returns in {result.RestoreSeconds}s."
                    : "The path stays on the clipboard.";
                var title = $"{(result.FromScreenshot ? "Screenshot" : "Image")} path copied — {result.Profile?.Label}";
                Report(new AppStatus(TrayState.Success, result.PathCopied ? $"{title}. {restore}" : result.Message, result.RemotePath));
                tray?.SetResult(true, title);
                NotifyUnlessWatching(title, result.PathCopied ? $"{result.RemotePath}\n{restore}" : result.Message, NotificationKind.Success);
                await RefreshUploadsIfOpenAsync();
                break;
            case UploadResultKind.Cancelled:
                tray?.SetIdle();
                Report(new AppStatus(TrayState.Idle, result.Message));
                break;
            case UploadResultKind.NoDestination:
                tray?.SetIdle();
                Report(new AppStatus(TrayState.Idle, result.Message));
                ShowProfiles(startAdding: Catalog?.Usable.Any() != true);
                break;
            case UploadResultKind.Busy:
                break;
            default:
                ReportFailure(result.Message);
                break;
        }
        return result;
    }

    private void ReportFailure(string message)
    {
        Report(new AppStatus(TrayState.Error, message));
        tray?.SetResult(false, message);
        NotifyUnlessWatching("Upload failed", message, NotificationKind.Error);
    }

    private void NotifyUnlessWatching(string title, string text, NotificationKind kind)
    {
        if (mainWindow is { IsVisible: true, IsActive: true })
        {
            return;
        }
        tray?.Notify(title, text, kind);
    }

    private void Report(AppStatus status)
    {
        Status = status;
        StatusChanged?.Invoke(this, EventArgs.Empty);
    }

    private async Task RefreshUploadsIfOpenAsync()
    {
        if (uploadsWindow is { IsVisible: true } window)
        {
            await window.RefreshAsync();
        }
    }

    public void ShowMain() => mainWindow?.ShowAndActivate();

    public void ShowProfiles(bool startAdding = false)
    {
        if (profilesWindow is null || !profilesWindow.IsLoaded)
        {
            profilesWindow = new ProfilesWindow(this);
            profilesWindow.Closed += (_, _) => profilesWindow = null;
        }
        profilesWindow.ShowAndActivate(startAdding);
    }

    public void ShowUploads()
    {
        if (uploadsWindow is null || !uploadsWindow.IsLoaded)
        {
            uploadsWindow = new UploadsWindow(this);
            uploadsWindow.Closed += (_, _) => uploadsWindow = null;
        }
        uploadsWindow.ShowAndActivate();
    }

    /// <summary>Hides app windows so a capture does not include them.</summary>
    private IDisposable HideWindowsForCapture()
    {
        var hidden = System.Windows.Application.Current.Windows
            .OfType<Window>()
            .Where(window => window.IsVisible)
            .ToList();
        foreach (var window in hidden)
        {
            window.Hide();
        }
        return new Restore(() =>
        {
            foreach (var window in hidden)
            {
                window.Show();
            }
        });
    }

    private FileSystemWatcher? CreateWatcher()
    {
        try
        {
            Directory.CreateDirectory(AppPaths.ConfigRoot);
            var created = new FileSystemWatcher(AppPaths.ConfigRoot)
            {
                IncludeSubdirectories = true,
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite | NotifyFilters.DirectoryName,
            };
            FileSystemEventHandler changed = (_, args) =>
            {
                if (args.FullPath.Contains($"{Path.DirectorySeparatorChar}logs", StringComparison.OrdinalIgnoreCase)
                    || args.Name?.EndsWith(".lock", StringComparison.OrdinalIgnoreCase) == true)
                {
                    return;
                }
                dispatcher.BeginInvoke(() =>
                {
                    catalogDebounce.Stop();
                    catalogDebounce.Start();
                });
            };
            created.Changed += changed;
            created.Created += changed;
            created.Deleted += changed;
            created.Renamed += (sender, args) => changed(sender, args);
            created.EnableRaisingEvents = true;
            return created;
        }
        catch (Exception exception)
        {
            Log.Warn($"Profile watcher unavailable: {exception.Message}");
            return null;
        }
    }

    public void Dispose()
    {
        watcher?.Dispose();
        catalogDebounce.Stop();
        tray?.Dispose();
        tray = null;
    }

    private sealed class Restore(Action action) : IDisposable
    {
        private Action? pending = action;

        public void Dispose()
        {
            Interlocked.Exchange(ref pending, null)?.Invoke();
        }
    }
}
