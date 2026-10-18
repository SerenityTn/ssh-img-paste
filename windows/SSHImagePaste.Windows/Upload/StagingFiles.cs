using System.Security.Cryptography;
using SSHImagePaste.Windows.Platform;

namespace SSHImagePaste.Windows.Upload;

/// <summary>
/// Private, uniquely named PNG files handed to the CLI. The CLI copies the
/// bytes into its own staging before any network step, so these files only
/// need to live for the duration of one upload.
/// </summary>
public static class StagingFiles
{
    private static readonly TimeSpan StaleAfter = TimeSpan.FromDays(1);

    public static string Write(byte[] png)
    {
        Directory.CreateDirectory(AppPaths.StagingDirectory);
        var name = $"upload-{Convert.ToHexStringLower(RandomNumberGenerator.GetBytes(12))}.png";
        var path = Path.Combine(AppPaths.StagingDirectory, name);
        using (var stream = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.Read, 81920, FileOptions.WriteThrough))
        {
            stream.Write(png);
        }
        return path;
    }

    public static void Delete(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch (Exception exception)
        {
            Log.Warn($"Could not delete staging file: {exception.Message}");
        }
    }

    /// <summary>Removes files left behind by a crash or forced shutdown.</summary>
    public static void RemoveStale()
    {
        try
        {
            if (!Directory.Exists(AppPaths.StagingDirectory))
            {
                return;
            }
            foreach (var file in Directory.EnumerateFiles(AppPaths.StagingDirectory, "upload-*.png"))
            {
                if (DateTime.UtcNow - File.GetLastWriteTimeUtc(file) > StaleAfter)
                {
                    File.Delete(file);
                }
            }
        }
        catch (Exception exception)
        {
            Log.Warn($"Staging cleanup: {exception.Message}");
        }
    }
}
