using System.Text.Json;

namespace SSHImagePaste.Windows.Cli;

public enum ShotMode
{
    Region,
    Full,
}

public sealed record ProfileProblem(string Code, string? Field, string? Key);

public sealed record ProfileInfo(
    string Id,
    bool Usable,
    bool Active,
    string Label,
    string Host,
    string RemoteHome,
    string RemoteDirectory,
    ShotMode ShotMode,
    int RestoreSeconds,
    bool Editable,
    ProfileProblem? Problem)
{
    public const int DefaultRestoreSeconds = 60;

    public string DisplayName => Usable ? Label : Id;

    public string RemoteRoot => $"{RemoteHome.TrimEnd('/')}/{RemoteDirectory.TrimEnd('/')}";
}

public sealed record ProfileCatalog(
    bool Configured,
    string ConfigRoot,
    string? ActiveProfileId,
    IReadOnlyList<ProfileInfo> Profiles)
{
    public ProfileInfo? Active => Profiles.FirstOrDefault(profile => profile.Id == ActiveProfileId && profile.Usable);

    public IEnumerable<ProfileInfo> Usable => Profiles.Where(profile => profile.Usable);
}

public sealed record ProfileFields(
    string? Label = null,
    string? Host = null,
    string? RemoteHome = null,
    string? RemoteDirectory = null,
    ShotMode? ShotMode = null,
    int? RestoreSeconds = null)
{
    public IEnumerable<string> ToArguments()
    {
        if (Label is not null) { yield return "--label"; yield return Label; }
        if (Host is not null) { yield return "--host"; yield return Host; }
        if (RemoteHome is not null) { yield return "--remote-home"; yield return RemoteHome; }
        if (RemoteDirectory is not null) { yield return "--remote-dir"; yield return RemoteDirectory; }
        if (ShotMode is { } mode) { yield return "--shot-mode"; yield return mode == Cli.ShotMode.Full ? "full" : "region"; }
        if (RestoreSeconds is { } seconds) { yield return "--restore-seconds"; yield return seconds.ToString(System.Globalization.CultureInfo.InvariantCulture); }
    }
}

public sealed record ConnectionTestResult(string Host, string ConfiguredHome, string DetectedHome, bool HomeMatches);

public sealed record RemoteUpload(string Name, long SizeBytes, DateTimeOffset Modified, string RemotePath);

public sealed record UploadListing(string RemoteDirectory, IReadOnlyList<RemoteUpload> Uploads, int SkippedRows, bool Truncated);

public sealed record CliVersion(string Version, string Containment, string? SshProgram);

internal static class Json
{
    public static string String(JsonElement element, string name, string fallback = "") =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? fallback
            : fallback;

    public static string? OptionalString(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

    public static bool Bool(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.True;

    public static long Long(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.Number && value.TryGetInt64(out var number)
            ? number
            : 0;

    public static IEnumerable<JsonElement> Array(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.Array
            ? value.EnumerateArray()
            : [];

    public static ProfileInfo Profile(JsonElement element)
    {
        var usable = Bool(element, "usable");
        ProfileProblem? problem = null;
        if (element.TryGetProperty("problem", out var problemElement) && problemElement.ValueKind == JsonValueKind.Object)
        {
            problem = new ProfileProblem(
                String(problemElement, "code", "unknown"),
                OptionalString(problemElement, "field"),
                OptionalString(problemElement, "key"));
        }
        var restore = OptionalString(element, "restore_seconds");
        return new ProfileInfo(
            String(element, "id"),
            usable,
            Bool(element, "active"),
            String(element, "label", String(element, "id")),
            String(element, "host"),
            String(element, "remote_home"),
            String(element, "remote_dir"),
            OptionalString(element, "shot_mode") == "full" ? ShotMode.Full : ShotMode.Region,
            int.TryParse(restore, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out var seconds)
                ? seconds
                : ProfileInfo.DefaultRestoreSeconds,
            Bool(element, "editable"),
            problem);
    }
}
