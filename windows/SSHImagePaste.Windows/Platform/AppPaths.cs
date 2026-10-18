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

    /// <summary>
    /// Shortens a path under the user's profile to an environment-variable
    /// form, so the UI never displays the account name.
    /// </summary>
    public static string DisplayPath(string path)
    {
        foreach (var (variable, folder) in new[]
                 {
                     ("%LOCALAPPDATA%", Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData)),
                     ("%USERPROFILE%", Environment.GetFolderPath(Environment.SpecialFolder.UserProfile)),
                 })
        {
            if (folder.Length > 0
                && path.StartsWith(folder, StringComparison.OrdinalIgnoreCase)
                && (path.Length == folder.Length || path[folder.Length] is '\\' or '/'))
            {
                return variable + path[folder.Length..];
            }
        }
        return path;
    }

    public static string SshConfigPath =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".ssh", "config");
}
