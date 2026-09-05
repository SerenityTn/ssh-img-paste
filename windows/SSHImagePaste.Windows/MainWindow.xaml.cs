using System.ComponentModel;
using System.Windows;

namespace SSHImagePaste.Windows;

public partial class MainWindow : System.Windows.Window
{
    private bool allowClose;

    public MainWindow()
    {
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
        StatusText.Text = action switch
        {
            DesktopAction.UploadClipboard => "Clipboard upload requested — Rust CLI connection is under development",
            DesktopAction.CaptureRegion => "Region capture requested — Windows capture adapter is under development",
            DesktopAction.CaptureFullScreen => "Full-screen capture requested — Windows capture adapter is under development",
            DesktopAction.ManageProfiles => "Profile manager requested — authoritative Rust persistence is under development",
            _ => StatusText.Text,
        };
        ShowAndActivate();
    }

    public void CloseForShutdown()
    {
        allowClose = true;
        Close();
    }

    protected override void OnClosing(CancelEventArgs e)
    {
        if (!allowClose)
        {
            e.Cancel = true;
            Hide();
            return;
        }
        base.OnClosing(e);
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
