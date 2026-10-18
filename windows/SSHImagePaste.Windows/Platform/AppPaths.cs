namespace SSHImagePaste.Windows.Platform;

/// <summary>Per-user locations owned by the Windows edition.</summary>
public static class AppPaths
{
    public const string ProductName = "SSH Image Paste";
    public const string CliFileName = "ssh-img-paste.exe";

    /// <summary>Absolute directory that replaces the default settings root, for tests and portable use.</summary>
    public const string ConfigRootEnvironmentVariable = "SSH_IMG_PASTE_CONFIG_ROOT";

    /// <summary>The settings root passed to the CLI, or null to use the CLI's default.</summary>
    public static string? ConfigRootOverride
    {
        get
        {
            var value = Environment.GetEnvironmentVariable(ConfigRootEnvironmentVariable);
            return !string.IsNullOrWhiteSpace(value) && Path.IsPathFullyQualified(value) ? value : null;
        }
    }

    /// <summary>Same root the Rust CLI uses for profiles.</summary>
    public static string ConfigRoot =>
        ConfigRootOverride
        ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), ProductName);

    public static string LogDirectory => Path.Combine(ConfigRoot, "logs");

    public static string StagingDirectory => Path.Combine(Path.GetTempPath(), ProductName, "staging");

    public static string ApplicationDirectory => AppContext.BaseDirectory;

    /// <summary>The bundled CLI that sits next to the desktop executable.</summary>
    public static string BundledCliPath => Path.Combine(ApplicationDirectory, CliFileName);

    public static string? ExecutablePath => Environment.ProcessPath;

    public static string SshConfigPath =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".ssh", "config");
}
