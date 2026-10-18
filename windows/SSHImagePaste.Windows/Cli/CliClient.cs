using System.Diagnostics;
using System.Text.Json;
using SSHImagePaste.Windows.Platform;

namespace SSHImagePaste.Windows.Cli;

/// <summary>A structured CLI failure (protocol v1 <c>error</c> object).</summary>
public sealed record CliError(
    string Code,
    string Message,
    string? Field = null,
    string? Step = null,
    string? Reason = null,
    int? ExitCode = null,
    string? Diagnostic = null);

public sealed class CliException(CliError error) : Exception(error.Message)
{
    public CliError Error { get; } = error;
}

public interface ICliRunner
{
    Task<JsonElement> RunAsync(IReadOnlyList<string> arguments, TimeSpan timeout, CancellationToken cancellationToken);
}

/// <summary>
/// Runs the authoritative Rust CLI with an argument array and parses its single
/// JSON document. The desktop shell never builds a command string and never
/// writes profile files itself.
/// </summary>
public sealed class CliClient(string executablePath, string? configRoot = null) : ICliRunner
{
    public const int ProtocolVersion = 1;
    private const int MaxOutputChars = 4 * 1024 * 1024;

    public string ExecutablePath { get; } = executablePath;

    public async Task<JsonElement> RunAsync(
        IReadOnlyList<string> arguments,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        if (!File.Exists(ExecutablePath))
        {
            throw new CliException(new CliError(
                "cli_missing",
                $"The SSH Image Paste command-line component is missing: {ExecutablePath}. Reinstall the app."));
        }

        var fullArguments = new List<string>(arguments.Count + 2);
        if (configRoot is not null)
        {
            fullArguments.Add("--config-root");
            fullArguments.Add(configRoot);
        }
        fullArguments.AddRange(arguments);

        using var process = new Process { StartInfo = CliInvocation.Create(ExecutablePath, fullArguments) };
        try
        {
            process.Start();
        }
        catch (Exception exception)
        {
            Log.Error("CLI start failed", exception);
            throw new CliException(new CliError("cli_start_failed", "The SSH Image Paste command-line component could not start."));
        }
        process.StandardInput.Close();

        var stdoutTask = ReadBoundedAsync(process.StandardOutput);
        var stderrTask = ReadBoundedAsync(process.StandardError);
        using var timeoutSource = new CancellationTokenSource(timeout);
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, timeoutSource.Token);
        try
        {
            await process.WaitForExitAsync(linked.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            KillTree(process);
            if (cancellationToken.IsCancellationRequested)
            {
                throw;
            }
            throw new CliException(new CliError(
                "cli_timeout",
                "The operation took too long and was stopped.",
                Reason: "timeout"));
        }

        var stdout = await stdoutTask.ConfigureAwait(false);
        var stderr = await stderrTask.ConfigureAwait(false);
        if (stderr.Length > 0)
        {
            Log.Warn($"CLI stderr ({arguments.FirstOrDefault()}): {stderr}");
        }
        return ParseDocument(stdout, process.ExitCode, arguments);
    }

    public static JsonElement ParseDocument(string stdout, int exitCode, IReadOnlyList<string> arguments)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(stdout.Trim());
        }
        catch (JsonException)
        {
            Log.Error($"CLI returned non-JSON output (exit {exitCode}) for {string.Join(' ', arguments.Take(2))}");
            throw new CliException(new CliError("cli_protocol", "The command-line component returned an unexpected response."));
        }
        using (document)
        {
            var root = document.RootElement.Clone();
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("protocol_version", out var version)
                || version.ValueKind != JsonValueKind.Number
                || version.GetInt32() != ProtocolVersion
                || !root.TryGetProperty("ok", out var ok))
            {
                throw new CliException(new CliError("cli_protocol", "The command-line component speaks an unsupported protocol version."));
            }
            if (ok.ValueKind == JsonValueKind.True && exitCode == 0 && root.TryGetProperty("result", out var result))
            {
                return result;
            }
            if (root.TryGetProperty("error", out var error) && error.ValueKind == JsonValueKind.Object)
            {
                throw new CliException(ParseError(error));
            }
            throw new CliException(new CliError("cli_protocol", "The command-line component reported an inconsistent result."));
        }
    }

    private static CliError ParseError(JsonElement error)
    {
        static string? Text(JsonElement element, string name) =>
            element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

        int? exitCode = error.TryGetProperty("exit_code", out var code) && code.ValueKind == JsonValueKind.Number
            ? code.GetInt32()
            : null;
        return new CliError(
            Text(error, "code") ?? "unknown",
            Text(error, "message") ?? "The operation failed.",
            Text(error, "field"),
            Text(error, "step"),
            Text(error, "reason"),
            exitCode,
            Text(error, "diagnostic"));
    }

    private static async Task<string> ReadBoundedAsync(StreamReader reader)
    {
        var buffer = new char[8192];
        var builder = new System.Text.StringBuilder();
        int read;
        while ((read = await reader.ReadAsync(buffer).ConfigureAwait(false)) > 0)
        {
            var remaining = MaxOutputChars - builder.Length;
            if (remaining > 0)
            {
                builder.Append(buffer, 0, Math.Min(read, remaining));
            }
        }
        return builder.ToString();
    }

    private static void KillTree(Process process)
    {
        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
                process.WaitForExit(5000);
            }
        }
        catch (Exception exception)
        {
            Log.Warn($"CLI teardown: {exception.Message}");
        }
    }
}
