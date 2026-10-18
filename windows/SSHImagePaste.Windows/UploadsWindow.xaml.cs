using System.Diagnostics;
using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Platform;

namespace SSHImagePaste.Windows;

public sealed record UploadRow(RemoteUpload Upload)
{
    public string Name => Upload.Name;

    public string Size => FormatSize(Upload.SizeBytes);

    public string Modified => Upload.Modified.ToLocalTime().ToString("g", CultureInfo.CurrentCulture);

    public static string FormatSize(long bytes) => bytes switch
    {
        < 1024 => $"{bytes} B",
        < 1024 * 1024 => $"{bytes / 1024.0:0.#} KB",
        _ => $"{bytes / (1024.0 * 1024):0.#} MB",
    };
}

public partial class UploadsWindow : Window
{
    private readonly AppController controller;
    private CancellationTokenSource? loading;
    private bool updatingDestinations;
    private bool working;

    public UploadsWindow(AppController controller)
    {
        this.controller = controller;
        InitializeComponent();
        controller.CatalogChanged += OnCatalogChanged;
        Closed += (_, _) =>
        {
            controller.CatalogChanged -= OnCatalogChanged;
            loading?.Cancel();
        };
        PopulateDestinations();
        UpdateButtons();
    }

    private ProfileInfo? Destination => DestinationBox.SelectedItem as ProfileInfo;

    private RemoteUpload? SelectedUpload => (UploadList.SelectedItem as UploadRow)?.Upload;

    public void ShowAndActivate()
    {
        Show();
        if (WindowState == WindowState.Minimized)
        {
            WindowState = WindowState.Normal;
        }
        Activate();
        _ = RefreshAsync();
    }

    private void OnCatalogChanged(object? sender, EventArgs e) => PopulateDestinations();

    private void PopulateDestinations()
    {
        updatingDestinations = true;
        try
        {
            var previous = Destination?.Id;
            var usable = controller.Catalog?.Usable.ToList() ?? [];
            DestinationBox.ItemsSource = usable;
            DestinationBox.SelectedItem = usable.FirstOrDefault(profile => profile.Id == previous)
                ?? usable.FirstOrDefault(profile => profile.Active)
                ?? usable.FirstOrDefault();
        }
        finally
        {
            updatingDestinations = false;
        }
    }

    public async Task RefreshAsync()
    {
        if (Destination is not { } destination)
        {
            UploadList.ItemsSource = null;
            StatusText.Text = "Add a destination to see its uploads.";
            UpdateButtons();
            return;
        }
        loading?.Cancel();
        var source = new CancellationTokenSource();
        loading = source;
        SetWorking(true, $"Loading uploads from {destination.Label}…");
        try
        {
            var listing = await controller.Service.ListUploadsAsync(destination.Id, source.Token);
            if (source.IsCancellationRequested)
            {
                return;
            }
            UploadList.ItemsSource = listing.Uploads.Select(upload => new UploadRow(upload)).ToList();
            var total = listing.Uploads.Sum(upload => upload.SizeBytes);
            StatusText.Text = listing.Uploads.Count == 0
                ? $"No uploads in {listing.RemoteDirectory} yet."
                : $"{listing.Uploads.Count} image(s), {UploadRow.FormatSize(total)}, in {listing.RemoteDirectory}."
                  + (listing.Truncated ? " Showing the newest only." : string.Empty)
                  + (listing.SkippedRows > 0 ? $" {listing.SkippedRows} file(s) with unsupported names are hidden." : string.Empty);
        }
        catch (OperationCanceledException)
        {
        }
        catch (CliException exception)
        {
            if (!source.IsCancellationRequested)
            {
                UploadList.ItemsSource = null;
                StatusText.Text = ErrorMessages.Describe(exception.Error);
            }
        }
        finally
        {
            if (loading == source)
            {
                SetWorking(false);
            }
        }
    }

    private void SetWorking(bool value, string? message = null)
    {
        working = value;
        Cursor = value ? Cursors.AppStarting : null;
        if (message is not null)
        {
            StatusText.Text = message;
        }
        UpdateButtons();
    }

    private void UpdateButtons()
    {
        var hasDestination = Destination is not null;
        var hasSelection = SelectedUpload is not null;
        RefreshButton.IsEnabled = hasDestination;
        OpenButton.IsEnabled = hasSelection && !working;
        CopyButton.IsEnabled = hasSelection;
        CleanButton.IsEnabled = hasDestination && !working && UploadList.Items.Count > 0;
        DestinationBox.IsEnabled = !working || DestinationBox.Items.Count > 1;
    }

    private void Destination_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!updatingDestinations)
        {
            _ = RefreshAsync();
        }
    }

    private void Refresh_Click(object sender, RoutedEventArgs e) => _ = RefreshAsync();

    private void UploadList_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateButtons();

    private void UploadList_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (OpenButton.IsEnabled)
        {
            Open_Click(sender, e);
        }
    }

    private async void Open_Click(object sender, RoutedEventArgs e)
    {
        if (Destination is not { } destination || SelectedUpload is not { } upload)
        {
            return;
        }
        SetWorking(true, $"Downloading {upload.Name}…");
        try
        {
            var localPath = await controller.Service.FetchUploadAsync(destination.Id, upload.Name);
            if (!localPath.EndsWith(".png", StringComparison.OrdinalIgnoreCase) || !File.Exists(localPath))
            {
                StatusText.Text = "The download did not produce a PNG file.";
                return;
            }
            Process.Start(new ProcessStartInfo(localPath) { UseShellExecute = true });
            StatusText.Text = $"Opened {upload.Name}.";
        }
        catch (CliException exception)
        {
            StatusText.Text = ErrorMessages.Describe(exception.Error);
        }
        catch (Exception exception)
        {
            Log.Error("Opening a downloaded image failed", exception);
            StatusText.Text = $"Downloaded, but Windows could not open it: {exception.Message}";
        }
        finally
        {
            SetWorking(false);
        }
    }

    private void Copy_Click(object sender, RoutedEventArgs e)
    {
        if (SelectedUpload is { } upload && controller.Clipboard.SetText(upload.RemotePath) is not null)
        {
            StatusText.Text = $"Copied {upload.RemotePath}";
        }
    }

    private async void Clean_Click(object sender, RoutedEventArgs e)
    {
        if (Destination is not { } destination)
        {
            return;
        }
        var count = UploadList.Items.Count;
        var answer = System.Windows.MessageBox.Show(
            this,
            $"Delete all {count} uploaded PNG image(s) from {destination.RemoteRoot} on {destination.Host}?\n\nThis cannot be undone. Other files in the folder are not touched.",
            "Delete all uploads",
            MessageBoxButton.OKCancel,
            MessageBoxImage.Warning,
            MessageBoxResult.Cancel);
        if (answer != MessageBoxResult.OK)
        {
            return;
        }
        SetWorking(true, $"Deleting uploads on {destination.Host}…");
        try
        {
            var deleted = await controller.Service.CleanUploadsAsync(destination.Id);
            StatusText.Text = $"Deleted {deleted} image(s) on {destination.Label}.";
        }
        catch (CliException exception)
        {
            StatusText.Text = ErrorMessages.Describe(exception.Error);
        }
        finally
        {
            SetWorking(false);
        }
        var message = StatusText.Text;
        await RefreshAsync();
        StatusText.Text = message;
    }
}
