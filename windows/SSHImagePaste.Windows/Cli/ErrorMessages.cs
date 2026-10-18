namespace SSHImagePaste.Windows.Cli;

/// <summary>Turns protocol errors into short, actionable messages.</summary>
public static class ErrorMessages
{
    public static string FieldLabel(string? field) => field switch
    {
        "label" => "Display name",
        "host" => "SSH host or alias",
        "remote_home" => "Remote home",
        "remote_dir" => "Upload folder",
        "shot_mode" => "Screenshot fallback",
        "restore_seconds" => "Clipboard restore delay",
        _ => "A field",
    };

    public static string Describe(CliError error) => error.Code switch
    {
        "connection_failed" or "upload_failed" or "remote_command_failed" => DescribeRemote(error),
        "invalid_field" or "profile_validation_failed" =>
            $"{FieldLabel(error.Field)} is invalid. {FieldHint(error.Field)}".TrimEnd(),
        "missing_field" => $"{FieldLabel(error.Field)} is required.",
        "profile_exists" => "A destination with that ID already exists. Choose another ID.",
        "profile_not_found" => "That destination no longer exists. Refresh and try again.",
        "profile_not_editable" =>
            "This destination was written by hand and is read-only here. Edit the file directly or duplicate it.",
        "profile_unusable" => "That destination is invalid. Fix or delete it first.",
        "profile_parse_failed" =>
            "That destination uses shell syntax such as $VARIABLES, which is never evaluated. Use literal values.",
        "last_usable_profile" => "You cannot delete your only working destination. Add another one first.",
        "switch_to_required" or "switch_to_same_profile" => "Choose a different destination to make active first.",
        "not_configured" => "No destination is set up yet. Add one in Manage Destinations.",
        "invalid_active_profile" => "The active destination setting is damaged. Choose a destination again.",
        "profile_store_unsafe" =>
            "The settings folder contains a link or unexpected file, so nothing was changed.",
        "profile_store_io" or "profile_store_partial" =>
            "The destination settings could not be saved. Check that the settings folder is writable.",
        "remote_home_unusable" =>
            "The host connected but reported an unusual home directory. Enter the remote home manually.",
        "fetched_not_png" => "The downloaded file was not a PNG image, so it was discarded.",
        "remote_output_invalid" => "The host returned an unexpected response.",
        "cli_timeout" => "The operation took too long and was stopped. Check the network and try again.",
        "cli_missing" or "cli_start_failed" or "cli_protocol" => error.Message,
        "source_not_png" => "The image could not be prepared as a PNG.",
        _ => error.Message,
    };

    private static string FieldHint(string? field) => field switch
    {
        "host" => "Use an alias from your SSH config or user@host, without spaces or ports.",
        "remote_home" => "Use an absolute Linux path such as /home/me.",
        "remote_dir" => "Use a folder relative to the remote home, such as img-uploads.",
        "restore_seconds" => "Use a whole number of seconds from 0 to 86400.",
        "label" => "Remove line breaks and control characters.",
        _ => string.Empty,
    };

    private static string DescribeRemote(CliError error)
    {
        var diagnostic = error.Diagnostic ?? string.Empty;
        var hint = SshHint(diagnostic, error);
        return hint ?? (string.IsNullOrWhiteSpace(diagnostic)
            ? error.Message
            : $"{error.Message} {Truncate(diagnostic.Trim(), 240)}");
    }

    /// <summary>Recognizes the OpenSSH failures users hit most often.</summary>
    public static string? SshHint(string diagnostic, CliError error)
    {
        bool Has(string text) => diagnostic.Contains(text, StringComparison.OrdinalIgnoreCase);

        if (error.Reason == "spawn")
        {
            return "OpenSSH was not found. Install the Windows \"OpenSSH Client\" optional feature in Settings > System > Optional features.";
        }
        if (error.Reason == "timeout")
        {
            return "The SSH host did not respond in time. Check the network or VPN.";
        }
        if (Has("Host key verification failed"))
        {
            return "The host key is not trusted yet. Run \"ssh <host>\" once in a terminal and accept the key, then retry.";
        }
        if (Has("REMOTE HOST IDENTIFICATION HAS CHANGED"))
        {
            return "The host key changed. Verify the server, then remove the old key with \"ssh-keygen -R <host>\".";
        }
        if (Has("Permission denied"))
        {
            return "The SSH key was rejected. Load your key into the Windows ssh-agent or set IdentityFile in your SSH config. Passphrase prompts are not supported.";
        }
        if (Has("Could not resolve hostname") || Has("No such host"))
        {
            return "The host name could not be resolved. Check the alias in your SSH config.";
        }
        if (Has("Connection refused"))
        {
            return "The host refused the SSH connection. Check the port in your SSH config.";
        }
        if (Has("timed out"))
        {
            return "The SSH connection timed out. Check the network or VPN.";
        }
        if (Has("unknown predicate") || Has("-printf"))
        {
            return "Listing uploads requires GNU find on the remote host.";
        }
        if (Has("No such file or directory") && error.Step == "fetch")
        {
            return "That upload no longer exists on the host.";
        }
        return null;
    }

    private static string Truncate(string value, int length) =>
        value.Length <= length ? value : value[..length] + "…";
}
