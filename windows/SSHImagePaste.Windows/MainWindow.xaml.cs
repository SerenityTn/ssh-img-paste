using System.ComponentModel;
using System.Windows;
using System.Windows.Automation.Peers;

namespace SSHImagePaste.Windows;

public partial class MainWindow : System.Windows.Window
{
    private readonly Func<bool> trayAvailable;
    private readonly Action shutdownApplication;
    private bool allowClose;

    public MainWindow(Func<bool> trayAvailable, Action shutdownApplication)
    {
        this.trayAvailable = trayAvailable;
        this.shutdownApplication = shutdownApplication;
        InitializeComponent();
    }

    public void ShowAndActivate()
    {
        Show();
        if (WindowState == WindowState.Minimized)
        {
            WindowState = WindowState.Normal;
        }
        Activate();
    }

    public void ReportAction(DesktopAction action)
    {
        SetStatus(action switch
        {
            DesktopAction.UploadClipboard => "Unavailable in this preview — Rust CLI connection is under development",
            DesktopAction.CaptureRegion => "Unavailable in this preview — Windows region capture is under development",
            DesktopAction.CaptureFullScreen => "Unavailable in this preview — Windows full-screen capture is under development",
            DesktopAction.ManageProfiles => "Unavailable in this preview — authoritative Rust profile persistence is under development",
            _ => StatusText.Text,
        });
        ShowAndActivate();
    }

    public void ReportTrayUnavailable()
    {
        SetStatus("Tray integration is unavailable. Closing this window will exit SSH Image Paste.");
        ShowAndActivate();
    }

    private void SetStatus(string status)
    {
        StatusText.Text = status;
        var peer = UIElementAutomationPeer.FromElement(StatusText)
            ?? UIElementAutomationPeer.CreatePeerForElement(StatusText);
        peer?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
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

        if (ClosePolicy.Decide(trayAvailable()) == CloseDecision.HideToTray)
        {
            e.Cancel = true;
            Hide();
            return;
        }

        allowClose = true;
        base.OnClosing(e);
        Dispatcher.BeginInvoke(shutdownApplication);
    }

    private void UploadClipboard_Click(object sender, RoutedEventArgs e) =>
        ReportAction(DesktopAction.UploadClipboard);

    private void CaptureRegion_Click(object sender, RoutedEventArgs e) =>
        ReportAction(DesktopAction.CaptureRegion);

    private void CaptureFullScreen_Click(object sender, RoutedEventArgs e) =>
        ReportAction(DesktopAction.CaptureFullScreen);

    private void ManageProfiles_Click(object sender, RoutedEventArgs e) =>
        ReportAction(DesktopAction.ManageProfiles);
}
