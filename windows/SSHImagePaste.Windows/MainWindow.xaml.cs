using System.ComponentModel;
using System.Diagnostics;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;
using SSHImagePaste.Windows.Upload;

namespace SSHImagePaste.Windows;

public partial class MainWindow : Window
{
    private readonly AppController controller;
    private bool allowClose;
    private bool updatingDestinations;
    private bool updatingStartup;

    public MainWindow(AppController controller)
    {
        this.controller = controller;
        InitializeComponent();
        AppImage.Source = IconFactory.Render(IconFactory.CreateAppIcon(), 128);
        VersionRun.Text = $"  ·  v{typeof(App).Assembly.GetName().Version?.ToString(3)}";
        controller.CatalogChanged += (_, _) => UpdateDestinations();
        controller.StatusChanged += (_, _) => UpdateStatus();
        UpdateDestinations();
        UpdateStatus();
        UpdateStartup();
    }

    public void ShowAndActivate()
    {
        Show();
        if (WindowState == WindowState.Minimized)
        {
            WindowState = WindowState.Normal;
        }
        Activate();
        UpdateStartup();
    }

    public void ReportTrayUnavailable()
    {
        SetStatus(TrayState.Error, "The notification-area icon is unavailable. Closing this window exits SSH Image Paste.");
    }

    private void UpdateDestinations()
    {
        updatingDestinations = true;
        try
        {
            var catalog = controller.Catalog;
            var usable = catalog?.Usable.ToList() ?? [];
            DestinationBox.ItemsSource = usable;
            DestinationBox.SelectedItem = usable.FirstOrDefault(profile => profile.Id == catalog?.ActiveProfileId);
            DestinationBox.IsEnabled = usable.Count > 1 && !controller.Uploads.IsBusy;
            var ready = catalog?.Active is not null && !controller.Uploads.IsBusy;
            UploadButton.IsEnabled = ready;
            RegionButton.IsEnabled = ready;
            FullButton.IsEnabled = ready;
            UploadsButton.IsEnabled = catalog?.Active is not null;
            if (controller.CatalogError is { } error)
            {
                SetStatus(TrayState.Error, error);
            }
            else if (usable.Count == 0 && catalog is not null)
            {
                SetStatus(TrayState.Idle, "No destination yet. Choose Manage… to add your SSH host.");
            }
        }
        finally
        {
            updatingDestinations = false;
        }
    }

    private void UpdateStatus()
    {
        var status = controller.Status;
        SetStatus(status.State, status.Message);
        if (status.RemotePath is { } path)
        {
            PathBox.Text = path;
            PathPanel.Visibility = Visibility.Visible;
        }
        UpdateDestinations();
    }

    private void SetStatus(TrayState state, string message)
    {
        StatusText.Text = message;
        StatusImage.Source = IconFactory.Render(IconFactory.CreateTrayIcon(state), 40);
        var peer = UIElementAutomationPeer.FromElement(StatusText) ?? UIElementAutomationPeer.CreatePeerForElement(StatusText);
        peer?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
    }

    private void UpdateStartup()
    {
        updatingStartup = true;
        try
        {
            if (StartupRegistration.IsPackaged)
            {
                StartupCheck.IsChecked = null;
                StartupCheck.IsEnabled = false;
                StartupCheck.Content = "Start at sign-in is managed in Settings > Apps > Startup";
            }
            else
            {
                StartupCheck.IsChecked = StartupRegistration.IsEnabled();
            }
        }
        finally
        {
            updatingStartup = false;
        }
    }

    public void CloseForShutdown()
    {
        allowClose = true;
        Close();
    }

    protected override void OnClosing(CancelEventArgs e)
    {
        if (allowClose)
        {
            base.OnClosing(e);
            return;
        }
        if (ClosePolicy.Decide(controller.TrayAvailable) == CloseDecision.HideToTray)
        {
            e.Cancel = true;
            Hide();
            return;
        }
        allowClose = true;
        base.OnClosing(e);
        Dispatcher.BeginInvoke(() => ((App)System.Windows.Application.Current).ShutdownApplication());
    }

    private async void Destination_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (updatingDestinations || DestinationBox.SelectedItem is not ProfileInfo profile || profile.Id == controller.Catalog?.ActiveProfileId)
        {
            return;
        }
        await controller.UseDestinationAsync(profile.Id);
    }

    private void Paste_Executed(object sender, ExecutedRoutedEventArgs e)
    {
        if (Keyboard.FocusedElement is TextBox { IsReadOnly: false })
        {
            return;
        }
        if (UploadButton.IsEnabled)
        {
            _ = controller.UploadAsync(UploadSource.Auto);
        }
    }

    private void Upload_Click(object sender, RoutedEventArgs e) => _ = controller.UploadAsync(UploadSource.Auto);

    private void Region_Click(object sender, RoutedEventArgs e) => _ = controller.UploadAsync(UploadSource.Region);

    private void Full_Click(object sender, RoutedEventArgs e) => _ = controller.UploadAsync(UploadSource.FullScreen);

    private void Uploads_Click(object sender, RoutedEventArgs e) => controller.ShowUploads();

    private void Manage_Click(object sender, RoutedEventArgs e) => controller.ShowProfiles();

    private void CopyPath_Click(object sender, RoutedEventArgs e)
    {
        if (!string.IsNullOrEmpty(PathBox.Text) && controller.Clipboard.SetText(PathBox.Text) is not null)
        {
            SetStatus(TrayState.Success, "Remote path copied again.");
        }
    }

    private void Startup_Changed(object sender, RoutedEventArgs e)
    {
        if (updatingStartup)
        {
            return;
        }
        try
        {
            StartupRegistration.SetEnabled(StartupCheck.IsChecked == true);
        }
        catch (Exception exception)
        {
            Log.Error("Startup registration failed", exception);
            SetStatus(TrayState.Error, "Could not change the sign-in setting.");
        }
        UpdateStartup();
    }

    private void OpenLogs_Click(object sender, RoutedEventArgs e)
    {
        Directory.CreateDirectory(AppPaths.LogDirectory);
        Process.Start(new ProcessStartInfo("explorer.exe") { ArgumentList = { AppPaths.LogDirectory }, UseShellExecute = false });
    }
}
