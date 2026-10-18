using System.Text;

namespace SSHImagePaste.Windows.Profiles;

/// <summary>
/// Client-side mirror of the core's profile rules for immediate form
/// feedback. The CLI re-validates everything and remains authoritative.
/// </summary>
public static class ProfileRules
{
    private const string ShellMeta = "'\"`$;&|<>(){}[]*?\\";

    public static bool IsValidId(string value) =>
        value.Length > 0
        && char.IsAsciiLetterOrDigit(value[0])
        && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '_' or '-');

    public static string? ValidateLabel(string value) =>
        value.Length == 0 ? "Enter a display name."
        : value.Any(IsControl) ? "Remove line breaks and control characters."
        : null;

    public static string? ValidateHost(string value)
    {
        if (value.Length == 0)
        {
            return "Enter an SSH host or alias.";
        }
        if (value.StartsWith('-'))
        {
            return "The host cannot start with a dash.";
        }
        if (value.Contains(':'))
        {
            return "Put ports in your SSH config instead of the host field.";
        }
        return value.All(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '_' or '@' or '-')
            ? null
            : "Use letters, digits, dots, dashes, underscores, and @ only.";
    }

    public static string? ValidateRemoteHome(string value)
    {
        if (value.Length == 0)
        {
            return "Enter the absolute home directory on the host.";
        }
        if (!value.StartsWith('/'))
        {
            return "Use an absolute Linux path such as /home/me.";
        }
        return IsSafePath(value) ? null : "Use letters, digits, dots, dashes, underscores, and / only.";
    }

    public static string? ValidateRemoteDirectory(string value)
    {
        if (value.Length == 0)
        {
            return "Enter an upload folder such as img-uploads.";
        }
        if (value.StartsWith('/'))
        {
            return "Use a folder relative to the remote home.";
        }
        if (value.StartsWith('-') || value == ".")
        {
            return "Choose a different folder name.";
        }
        return IsSafePath(value) ? null : "Use letters, digits, dots, dashes, underscores, and / only.";
    }

    public static string? ValidateRestoreSeconds(string value) =>
        int.TryParse(value, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out var seconds)
            && seconds is >= 0 and <= 86400
            ? null
            : "Use a whole number of seconds from 0 to 86400.";

    /// <summary>Derives a profile ID from a display name.</summary>
    public static string SuggestId(string label, IEnumerable<string> existingIds)
    {
        var builder = new StringBuilder();
        foreach (var character in label.Normalize(NormalizationForm.FormD))
        {
            if (char.IsAsciiLetterOrDigit(character))
            {
                builder.Append(char.ToLowerInvariant(character));
            }
            else if ((char.IsWhiteSpace(character) || character is '-' or '_' or '.') && builder.Length > 0 && builder[^1] != '-')
            {
                builder.Append('-');
            }
        }
        var baseId = builder.ToString().Trim('-');
        if (baseId.Length > 40)
        {
            baseId = baseId[..40].Trim('-');
        }
        if (baseId.Length == 0)
        {
            baseId = "destination";
        }
        var taken = new HashSet<string>(existingIds, StringComparer.OrdinalIgnoreCase);
        if (!taken.Contains(baseId))
        {
            return baseId;
        }
        for (var suffix = 2; ; suffix++)
        {
            var candidate = $"{baseId}-{suffix}";
            if (!taken.Contains(candidate))
            {
                return candidate;
            }
        }
    }

    private static bool IsControl(char c) => c <= '\u001f' || c == '\u007f';

    private static bool IsSafePath(string value) =>
        !value.Contains("//")
        && !value.Split('/').Any(part => part is "." or "..")
        && !value.Any(c => IsControl(c) || ShellMeta.Contains(c))
        && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '_' or '/' or '-');
}
