namespace SSHImagePaste.Windows.Cli;

/// <summary>Typed wrapper over the CLI protocol used by the desktop UI.</summary>
public sealed class SshImagePasteService(ICliRunner runner)
{
    private static readonly TimeSpan LocalTimeout = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan ProbeTimeout = TimeSpan.FromSeconds(40);
    private static readonly TimeSpan ListTimeout = TimeSpan.FromSeconds(50);
    private static readonly TimeSpan UploadTimeout = TimeSpan.FromSeconds(120);
    private static readonly TimeSpan FetchTimeout = TimeSpan.FromSeconds(150);
    private static readonly TimeSpan CleanTimeout = TimeSpan.FromSeconds(80);

    public async Task<ProfileCatalog> GetCatalogAsync(CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["profiles", "list", "--detailed"], LocalTimeout, cancellationToken).ConfigureAwait(false);
        return new ProfileCatalog(
            Json.Bool(result, "configured"),
            Json.String(result, "config_root"),
            Json.OptionalString(result, "active_profile_id"),
            Json.Array(result, "profiles").Select(Json.Profile).ToList());
    }

    public async Task<bool> CreateProfileAsync(string id, ProfileFields fields, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["profiles", "create", id, .. fields.ToArguments()], LocalTimeout, cancellationToken).ConfigureAwait(false);
        return Json.Bool(result, "activated");
    }

    public Task UpdateProfileAsync(string id, ProfileFields fields, CancellationToken cancellationToken = default) =>
        runner.RunAsync(["profiles", "update", id, .. fields.ToArguments()], LocalTimeout, cancellationToken);

    public Task RenameProfileAsync(string oldId, string newId, CancellationToken cancellationToken = default) =>
        runner.RunAsync(["profiles", "rename", oldId, newId], LocalTimeout, cancellationToken);

    public Task UseProfileAsync(string id, CancellationToken cancellationToken = default) =>
        runner.RunAsync(["profiles", "use", id], LocalTimeout, cancellationToken);

    public Task DeleteProfileAsync(string id, string? switchTo, CancellationToken cancellationToken = default) =>
        runner.RunAsync(
            switchTo is null ? ["profiles", "delete", id] : ["profiles", "delete", id, "--switch-to", switchTo],
            LocalTimeout,
            cancellationToken);

    public async Task<ConnectionTestResult> TestProfileAsync(string id, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["profiles", "test", id], ProbeTimeout, cancellationToken).ConfigureAwait(false);
        return new ConnectionTestResult(
            Json.String(result, "host"),
            Json.String(result, "remote_home"),
            Json.String(result, "detected_remote_home"),
            Json.Bool(result, "remote_home_matches"));
    }

    public async Task<string> ProbeHostAsync(string host, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["hosts", "probe", "--host", host], ProbeTimeout, cancellationToken).ConfigureAwait(false);
        return Json.String(result, "detected_remote_home");
    }

    public async Task<string> UploadFileAsync(string sourcePath, string profileId, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(
            ["upload-file", "--source", sourcePath, "--profile", profileId, "--confirm-profile", profileId],
            UploadTimeout,
            cancellationToken).ConfigureAwait(false);
        return Json.String(result, "remote_path");
    }

    public async Task<UploadListing> ListUploadsAsync(string profileId, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["uploads", "list", "--profile", profileId], ListTimeout, cancellationToken).ConfigureAwait(false);
        var uploads = Json.Array(result, "uploads")
            .Select(upload => new RemoteUpload(
                Json.String(upload, "name"),
                Json.Long(upload, "size_bytes"),
                DateTimeOffset.FromUnixTimeSeconds(Json.Long(upload, "modified_epoch_seconds")),
                Json.String(upload, "remote_path")))
            .ToList();
        return new UploadListing(
            Json.String(result, "remote_directory"),
            uploads,
            (int)Json.Long(result, "skipped_rows"),
            Json.Bool(result, "truncated"));
    }

    public async Task<string> FetchUploadAsync(string profileId, string name, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["uploads", "fetch", "--profile", profileId, "--name", name], FetchTimeout, cancellationToken).ConfigureAwait(false);
        return Json.String(result, "local_path");
    }

    public async Task<long> CleanUploadsAsync(string profileId, CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(
            ["uploads", "clean", "--profile", profileId, "--confirm-profile", profileId],
            CleanTimeout,
            cancellationToken).ConfigureAwait(false);
        return Json.Long(result, "deleted_count");
    }

    public async Task<CliVersion> GetVersionAsync(CancellationToken cancellationToken = default)
    {
        var result = await runner.RunAsync(["version"], LocalTimeout, cancellationToken).ConfigureAwait(false);
        return new CliVersion(
            Json.String(result, "version"),
            Json.String(result, "process_containment"),
            Json.OptionalString(result, "ssh_program"));
    }
}
