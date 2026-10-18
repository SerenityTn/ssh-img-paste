namespace SSHImagePaste.Windows.Platform;

/// <summary>Per-user locations owned by the Windows edition.</summary>
public static class AppPaths
{
    public const string ProductName = "SSH Image Paste";
    public const string CliFileName = "ssh-img-paste.exe";

    /// <summary>Same root the Rust CLI uses for profiles.</summary>
    public static string ConfigRoot =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), ProductName);

    public static string LogDirectory => Path.Combine(ConfigRoot, "logs");

    public static string StagingDirectory => Path.Combine(Path.GetTempPath(), ProductName, "staging");

    public static string ApplicationDirectory => AppContext.BaseDirectory;

    /// <summary>The bundled CLI that sits next to the desktop executable.</summary>
    public static string BundledCliPath => Path.Combine(ApplicationDirectory, CliFileName);

    public static string? ExecutablePath => Environment.ProcessPath;

    public static string SshConfigPath =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".ssh", "config");
}
