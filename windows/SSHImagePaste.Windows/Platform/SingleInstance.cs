namespace SSHImagePaste.Windows.Platform;

/// <summary>
/// Keeps one tray instance per user session. A second launch signals the
/// first one to show its window and then exits.
/// </summary>
public sealed class SingleInstance : IDisposable
{
    private readonly Mutex mutex;
    private readonly EventWaitHandle activation;
    private readonly RegisteredWaitHandle? registration;

    private SingleInstance(Mutex mutex, EventWaitHandle activation, Action onActivate)
    {
        this.mutex = mutex;
        this.activation = activation;
        registration = ThreadPool.RegisterWaitForSingleObject(
            activation,
            (_, _) => onActivate(),
            null,
            Timeout.Infinite,
            executeOnlyOnce: false);
    }

    private static string Scope => $"{Environment.UserDomainName}.{Environment.UserName}".Replace('\\', '.');

    /// <summary>Returns null when another instance owns the session; that instance is asked to activate.</summary>
    public static SingleInstance? TryAcquire(Action onActivate)
    {
        var mutex = new Mutex(initiallyOwned: true, $@"Local\SSHImagePaste.{Scope}", out var created);
        var activation = new EventWaitHandle(false, EventResetMode.AutoReset, $@"Local\SSHImagePaste.Activate.{Scope}");
        if (!created)
        {
            activation.Set();
            activation.Dispose();
            mutex.Dispose();
            return null;
        }
        return new SingleInstance(mutex, activation, onActivate);
    }

    public void Dispose()
    {
        registration?.Unregister(null);
        activation.Dispose();
        try
        {
            mutex.ReleaseMutex();
        }
        catch (ApplicationException)
        {
        }
        mutex.Dispose();
    }
}
