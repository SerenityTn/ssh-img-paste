using System.Windows;
using System.Windows.Threading;
using SSHImagePaste.Windows.Platform;

namespace SSHImagePaste.Windows;

public partial class App : System.Windows.Application
{
    private SingleInstance? instance;
    private AppController? controller;
    private bool shuttingDown;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        DispatcherUnhandledException += OnUnhandledException;
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
            Log.Error("Unhandled exception", args.ExceptionObject as Exception);
        TaskScheduler.UnobservedTaskException += (_, args) =>
        {
            Log.Error("Unobserved task exception", args.Exception);
            args.SetObserved();
        };

        instance = SingleInstance.TryAcquire(() => Dispatcher.BeginInvoke(() => controller?.ShowMain()));
        if (instance is null)
        {
            Shutdown();
            return;
        }

        System.Windows.Forms.Integration.WindowsFormsHost.EnableWindowsFormsInterop();
        System.Windows.Forms.Application.EnableVisualStyles();
        var background = e.Args.Contains(StartupRegistration.BackgroundArgument, StringComparer.OrdinalIgnoreCase);
        Log.Info($"Starting {AppPaths.ProductName} {typeof(App).Assembly.GetName().Version} (background: {background}, packaged: {StartupRegistration.IsPackaged})");
        controller = new AppController(Dispatcher, ShutdownApplication);
        controller.Start(background);
    }

    private void OnUnhandledException(object sender, DispatcherUnhandledExceptionEventArgs e)
    {
        Log.Error("UI exception", e.Exception);
        e.Handled = true;
        System.Windows.MessageBox.Show(
            $"Something went wrong: {e.Exception.Message}\n\nDetails were written to {Log.FilePath}.",
            AppPaths.ProductName,
            MessageBoxButton.OK,
            MessageBoxImage.Warning);
    }

    public void ShutdownApplication()
    {
        if (shuttingDown)
        {
            return;
        }
        shuttingDown = true;
        foreach (var window in Windows.OfType<Window>().ToList())
        {
            if (window is MainWindow main)
            {
                main.CloseForShutdown();
            }
            else
            {
                window.Close();
            }
        }
        controller?.Dispose();
        controller = null;
        Shutdown();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        controller?.Dispose();
        instance?.Dispose();
        Log.Info("Exited");
        base.OnExit(e);
    }
}
