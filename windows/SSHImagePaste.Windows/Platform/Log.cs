using System.Text;

namespace SSHImagePaste.Windows.Platform;

/// <summary>
/// Small rolling diagnostic log under the config root. Logging never throws
/// into callers; a broken log must not break uploads.
/// </summary>
public static class Log
{
    private const long MaxBytes = 1024 * 1024;
    private static readonly object Gate = new();

    public static string FilePath => Path.Combine(AppPaths.LogDirectory, "app.log");

    public static void Info(string message) => Write("INFO", message);

    public static void Warn(string message) => Write("WARN", message);

    public static void Error(string message, Exception? exception = null) =>
        Write("ERROR", exception is null ? message : $"{message}: {exception.GetType().Name}: {exception.Message}");

    private static void Write(string level, string message)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(AppPaths.LogDirectory);
                var path = FilePath;
                if (File.Exists(path) && new FileInfo(path).Length > MaxBytes)
                {
                    File.Move(path, path + ".1", overwrite: true);
                }
                var line = $"{DateTimeOffset.Now:yyyy-MM-dd HH:mm:ss.fff zzz} {level} {Sanitize(message)}{Environment.NewLine}";
                File.AppendAllText(path, line, Encoding.UTF8);
            }
        }
        catch
        {
            // Diagnostics are best effort.
        }
    }

    private static string Sanitize(string message)
    {
        var builder = new StringBuilder(Math.Min(message.Length, 4000));
        foreach (var character in message)
        {
            if (builder.Length >= 4000)
            {
                break;
            }
            builder.Append(char.IsControl(character) ? ' ' : character);
        }
        return builder.ToString();
    }
}
