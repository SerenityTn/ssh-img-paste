using System.Windows;
using Forms = System.Windows.Forms;

namespace SSHImagePaste.Windows;

public partial class App : System.Windows.Application
{
    private Forms.NotifyIcon? trayIcon;
    private Forms.ContextMenuStrip? trayMenu;
    private MainWindow? window;
    private bool trayAvailable;
    private bool shuttingDown;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        window = new MainWindow(() => trayAvailable, ShutdownApplication);
        MainWindow = window;
        window.Show();

        try
        {
            trayMenu = BuildTrayMenu();
            trayIcon = new Forms.NotifyIcon
            {
                Text = "SSH Image Paste",
                Icon = System.Drawing.SystemIcons.Application,
                ContextMenuStrip = trayMenu,
                Visible = true,
            };
            trayAvailable = true;
            trayIcon.DoubleClick += (_, _) => Dispatch(TrayInteraction.ActionForDoubleClick());
        }
        catch (Exception)
        {
            DisposeTray();
            window.ReportTrayUnavailable();
        }
    }

    private Forms.ContextMenuStrip BuildTrayMenu()
    {
        var menu = new Forms.ContextMenuStrip();
        foreach (var action in TrayMenuModel.PrimaryActions)
        {
            if (action is DesktopAction.UploadClipboard or DesktopAction.ManageProfiles or DesktopAction.Quit)
            {
                menu.Items.Add(new Forms.ToolStripSeparator());
            }
            var item = new Forms.ToolStripMenuItem(TrayMenuModel.Label(action))
            {
                Enabled = action is DesktopAction.OpenWindow or DesktopAction.Quit,
            };
            item.Click += (_, _) => Dispatch(action);
            menu.Items.Add(item);
        }
        return menu;
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
        if (action == DesktopAction.OpenWindow)
        {
            window.ShowAndActivate();
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
        DisposeTray();
        window?.CloseForShutdown();
        Shutdown();
    }

    private void DisposeTray()
    {
        trayAvailable = false;
        if (trayIcon is not null)
        {
            trayIcon.Visible = false;
            trayIcon.Dispose();
            trayIcon = null;
        }
        trayMenu?.Dispose();
        trayMenu = null;
    }

    protected override void OnExit(ExitEventArgs e)
    {
        DisposeTray();
        base.OnExit(e);
    }
}
