using System.Diagnostics;
using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Platform;
using SSHImagePaste.Windows.Profiles;

namespace SSHImagePaste.Windows;

public enum ProfileEditorMode
{
    Create,
    Edit,
}

public partial class ProfileEditorWindow : Window
{
    private readonly SshImagePasteService service;
    private readonly ProfileEditorMode mode;
    private readonly ProfileInfo? original;
    private readonly IReadOnlyList<string> existingIds;
    private bool idEdited;
    private bool settingId;
    private bool attempted;
    private bool working;

    public ProfileEditorWindow(SshImagePasteService service, ProfileEditorMode mode, ProfileInfo? template, IReadOnlyList<string> existingIds)
    {
        this.service = service;
        this.mode = mode;
        original = mode == ProfileEditorMode.Edit ? template : null;
        this.existingIds = existingIds;
        InitializeComponent();
        SshConfigRun.Text = AppPaths.DisplayPath(AppPaths.SshConfigPath);

        var editing = mode == ProfileEditorMode.Edit;
        Heading.Text = editing ? $"Edit {template?.DisplayName}" : template is null ? "Add destination" : $"Duplicate {template.DisplayName}";
        Title = Heading.Text;
        IdPanel.Visibility = editing ? Visibility.Collapsed : Visibility.Visible;

        if (template is { Usable: true })
        {
            LabelBox.Text = editing ? template.Label : $"{template.Label} copy";
            HostBox.Text = template.Host;
            HomeBox.Text = template.RemoteHome;
            DirBox.Text = template.RemoteDirectory;
            ShotModeBox.SelectedIndex = template.ShotMode == ShotMode.Full ? 1 : 0;
            RestoreBox.Text = template.RestoreSeconds.ToString(CultureInfo.InvariantCulture);
        }
        else
        {
            DirBox.Text = "img-uploads";
            ShotModeBox.SelectedIndex = 0;
            RestoreBox.Text = ProfileInfo.DefaultRestoreSeconds.ToString(CultureInfo.InvariantCulture);
        }
        if (!editing)
        {
            SetId(ProfileRules.SuggestId(LabelBox.Text.Length > 0 ? LabelBox.Text : "destination", existingIds));
        }
        Loaded += (_, _) => (LabelBox.Text.Length == 0 ? LabelBox : (Control)HostBox).Focus();
        Validate();
    }

    public string? SavedId { get; private set; }

    public string? SavedMessage { get; private set; }

    private void SetId(string id)
    {
        settingId = true;
        IdBox.Text = id;
        settingId = false;
    }

    private void Id_Changed(object sender, TextChangedEventArgs e)
    {
        if (!settingId)
        {
            idEdited = true;
        }
        Validate();
    }

    private void Field_Changed(object sender, TextChangedEventArgs e)
    {
        if (sender == LabelBox && mode == ProfileEditorMode.Create && !idEdited)
        {
            SetId(ProfileRules.SuggestId(LabelBox.Text, existingIds));
        }
        Validate();
    }

    private bool Validate()
    {
        if (!IsInitialized || SaveButton is null)
        {
            return false;
        }
        var errors = new (TextBlock Target, string? Error)[]
        {
            (LabelError, ProfileRules.ValidateLabel(LabelBox.Text.Trim())),
            (IdError, mode == ProfileEditorMode.Create ? ValidateId(IdBox.Text.Trim()) : null),
            (HostError, ProfileRules.ValidateHost(HostBox.Text.Trim())),
            (HomeError, ProfileRules.ValidateRemoteHome(HomeBox.Text.Trim())),
            (DirError, ProfileRules.ValidateRemoteDirectory(DirBox.Text.Trim())),
            (RestoreError, ProfileRules.ValidateRestoreSeconds(RestoreBox.Text.Trim())),
        };
        foreach (var (target, error) in errors)
        {
            // Empty fields only show errors after a save attempt, so a new form is not all red.
            var show = error is not null && (attempted || !IsEmptyFieldError(target));
            target.Text = error ?? string.Empty;
            target.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        }
        var home = HomeBox.Text.Trim().TrimEnd('/');
        var dir = DirBox.Text.Trim().Trim('/');
        DirHint.Text = ProfileRules.ValidateRemoteHome(HomeBox.Text.Trim()) is null && ProfileRules.ValidateRemoteDirectory(DirBox.Text.Trim()) is null
            ? $"Uploads land in {home}/{dir}. The folder is created on first upload."
            : "Relative to the remote home, such as img-uploads.";
        DetectButton.IsEnabled = !working && ProfileRules.ValidateHost(HostBox.Text.Trim()) is null;
        SaveButton.IsEnabled = !working;
        return errors.All(entry => entry.Error is null);
    }

    private bool IsEmptyFieldError(TextBlock target)
    {
        var box = target == LabelError ? LabelBox
            : target == IdError ? IdBox
            : target == HostError ? HostBox
            : target == HomeError ? HomeBox
            : target == DirError ? DirBox
            : RestoreBox;
        return box.Text.Trim().Length == 0;
    }

    private string? ValidateId(string id)
    {
        if (!ProfileRules.IsValidId(id))
        {
            return "Start with a letter or digit, then use letters, digits, - or _.";
        }
        return existingIds.Contains(id, StringComparer.OrdinalIgnoreCase) ? "A destination with this ID already exists." : null;
    }

    private void ShowStatus(string? message, bool error = false)
    {
        StatusText.Text = message ?? string.Empty;
        StatusText.Visibility = message is null ? Visibility.Collapsed : Visibility.Visible;
        StatusText.Foreground = error
            ? new System.Windows.Media.SolidColorBrush(System.Windows.Media.Color.FromRgb(0xD1, 0x34, 0x38))
            : (System.Windows.Media.Brush)FindResource(SystemColors.ControlTextBrushKey);
    }

    private void SetWorking(bool value)
    {
        working = value;
        Cursor = value ? System.Windows.Input.Cursors.AppStarting : null;
        Validate();
    }

    private async void Detect_Click(object sender, RoutedEventArgs e)
    {
        var host = HostBox.Text.Trim();
        SetWorking(true);
        ShowStatus($"Connecting to {host}…");
        try
        {
            HomeBox.Text = await service.ProbeHostAsync(host);
            ShowStatus($"Connected. {host} reports home {HomeBox.Text}.");
        }
        catch (CliException exception)
        {
            ShowStatus(ErrorMessages.Describe(exception.Error), error: true);
        }
        finally
        {
            SetWorking(false);
        }
    }

    private async void Save_Click(object sender, RoutedEventArgs e)
    {
        attempted = true;
        if (!Validate())
        {
            ShowStatus("Fix the highlighted fields.", error: true);
            return;
        }
        var restoreSeconds = int.Parse(RestoreBox.Text.Trim(), NumberStyles.None, CultureInfo.InvariantCulture);
        var shotMode = (ShotModeBox.SelectedItem as ComboBoxItem)?.Tag as string == "full" ? ShotMode.Full : ShotMode.Region;
        var fields = new ProfileFields(
            LabelBox.Text.Trim(),
            HostBox.Text.Trim(),
            HomeBox.Text.Trim(),
            DirBox.Text.Trim(),
            shotMode,
            restoreSeconds);
        SetWorking(true);
        ShowStatus("Saving…");
        try
        {
            if (mode == ProfileEditorMode.Create)
            {
                var id = IdBox.Text.Trim();
                var activated = await service.CreateProfileAsync(id, fields);
                SavedId = id;
                SavedMessage = activated ? $"Added {fields.Label}. It is now the active destination." : $"Added {fields.Label}.";
            }
            else if (original is not null)
            {
                await service.UpdateProfileAsync(original.Id, ChangedFields(fields));
                SavedId = original.Id;
                SavedMessage = $"Saved {fields.Label}.";
            }
            DialogResult = true;
        }
        catch (CliException exception)
        {
            ShowStatus(ErrorMessages.Describe(exception.Error), error: true);
            FocusField(exception.Error.Field);
        }
        finally
        {
            if (IsLoaded && DialogResult != true)
            {
                SetWorking(false);
            }
        }
    }

    /// <summary>Only rewrites fields that changed, so hand-added lines and defaults stay untouched.</summary>
    private ProfileFields ChangedFields(ProfileFields fields)
    {
        if (original is not { Usable: true } before)
        {
            return fields;
        }
        var changed = new ProfileFields(
            fields.Label != before.Label ? fields.Label : null,
            fields.Host != before.Host ? fields.Host : null,
            fields.RemoteHome != before.RemoteHome ? fields.RemoteHome : null,
            fields.RemoteDirectory != before.RemoteDirectory ? fields.RemoteDirectory : null,
            fields.ShotMode != before.ShotMode ? fields.ShotMode : null,
            fields.RestoreSeconds != before.RestoreSeconds ? fields.RestoreSeconds : null);
        // Update needs at least one field; an unchanged save rewrites only the label.
        return changed == new ProfileFields() ? new ProfileFields(Label: fields.Label) : changed;
    }

    private void FocusField(string? field)
    {
        Control? target = field switch
        {
            "label" => LabelBox,
            "host" => HostBox,
            "remote_home" => HomeBox,
            "remote_dir" => DirBox,
            "restore_seconds" => RestoreBox,
            _ => null,
        };
        target?.Focus();
    }

    private void OpenSshConfig_Click(object sender, RoutedEventArgs e)
    {
        var path = AppPaths.SshConfigPath;
        var directory = Path.GetDirectoryName(path)!;
        if (File.Exists(path))
        {
            Process.Start(new ProcessStartInfo("notepad.exe") { ArgumentList = { path }, UseShellExecute = false });
        }
        else if (Directory.Exists(directory))
        {
            Process.Start(new ProcessStartInfo("explorer.exe") { ArgumentList = { directory }, UseShellExecute = false });
        }
        else
        {
            ShowStatus($"No SSH config yet. Create {path} to define host aliases.");
        }
    }
}
