using System.Windows;
using Forms = System.Windows.Forms;

namespace SSHImagePaste.Windows;

public partial class App : System.Windows.Application
{
    private Forms.NotifyIcon? trayIcon;
    private MainWindow? window;
    private bool shuttingDown;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        window = new MainWindow();
        MainWindow = window;
        window.Show();

        var menu = new Forms.ContextMenuStrip();
        foreach (var action in TrayMenuModel.PrimaryActions)
        {
            if (action is DesktopAction.ManageProfiles or DesktopAction.Quit)
            {
                menu.Items.Add(new Forms.ToolStripSeparator());
            }
            var item = new Forms.ToolStripMenuItem(TrayMenuModel.Label(action));
            item.Click += (_, _) => Dispatch(action);
            menu.Items.Add(item);
        }

        trayIcon = new Forms.NotifyIcon
        {
            Text = "SSH Image Paste",
            Icon = System.Drawing.SystemIcons.Application,
            ContextMenuStrip = menu,
            Visible = true,
        };
        trayIcon.MouseClick += (_, args) =>
        {
            if (args.Button == Forms.MouseButtons.Left)
            {
                Dispatch(DesktopAction.UploadClipboard);
            }
        };
        trayIcon.DoubleClick += (_, _) => window.ShowAndActivate();
    }

    private void Dispatch(DesktopAction action)
    {
        if (window is null)
        {
            return;
        }
        if (action == DesktopAction.Quit)
        {
            ShutdownApplication();
            return;
        }
        if (action == DesktopAction.ManageProfiles)
        {
            window.ShowAndActivate();
        }
        window.ReportAction(action);
    }

    private void ShutdownApplication()
    {
        if (shuttingDown)
        {
            return;
        }
        shuttingDown = true;
        if (trayIcon is not null)
        {
            trayIcon.Visible = false;
            trayIcon.Dispose();
            trayIcon = null;
        }
        window?.CloseForShutdown();
        Shutdown();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        trayIcon?.Dispose();
        base.OnExit(e);
    }
}
