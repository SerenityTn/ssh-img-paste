using System.Diagnostics;

namespace SSHImagePaste.Windows;

public static class CliInvocation
{
    public static ProcessStartInfo Create(string executable, IEnumerable<string> arguments)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(executable);

        var startInfo = new ProcessStartInfo
        {
            FileName = executable,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        foreach (var argument in arguments)
        {
            startInfo.ArgumentList.Add(argument);
        }
        return startInfo;
    }
}
