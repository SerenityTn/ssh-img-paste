using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Platform;
using SSHImagePaste.Windows.Profiles;

namespace SSHImagePaste.Windows;

public sealed record ProfileListItem(ProfileInfo Profile)
{
    public string Title => Profile.DisplayName;

    public string Subtitle => Profile.Usable ? $"{Profile.Id}  ·  {Profile.Host}" : $"{Profile.Id}  ·  {ProfilesWindow.DescribeProblem(Profile.Problem)}";

    public string Badge =>
        !Profile.Usable ? "Needs attention"
        : Profile.Active && !Profile.Editable ? "Active · Read-only"
        : Profile.Active ? "Active"
        : !Profile.Editable ? "Read-only"
        : string.Empty;
}

public partial class ProfilesWindow : Window
{
    private readonly AppController controller;
    private bool busy;
    private string? selectAfterRefresh;

    public ProfilesWindow(AppController controller)
    {
        this.controller = controller;
        InitializeComponent();
        controller.CatalogChanged += OnCatalogChanged;
        Closed += (_, _) => controller.CatalogChanged -= OnCatalogChanged;
        Populate();
    }

    private ProfileInfo? Selected => (ProfileList.SelectedItem as ProfileListItem)?.Profile;

    public void ShowAndActivate(bool startAdding)
    {
        Show();
        if (WindowState == WindowState.Minimized)
        {
            WindowState = WindowState.Normal;
        }
        Activate();
        _ = controller.RefreshCatalogAsync();
        if (startAdding)
        {
            Dispatcher.BeginInvoke(() => _ = AddAsync(null));
        }
    }

    public static string DescribeProblem(ProfileProblem? problem) => problem?.Code switch
    {
        "dynamic_syntax" => $"{problem.Key ?? "A field"} uses shell syntax that is never evaluated",
        "invalid_field" => $"{ErrorMessages.FieldLabel(problem.Field)} is invalid",
        "unreadable" => "file is a link or cannot be read",
        _ => "cannot be used",
    };

    private void OnCatalogChanged(object? sender, EventArgs e) => Populate();

    private void Populate()
    {
        var catalog = controller.Catalog;
        var previous = selectAfterRefresh ?? Selected?.Id ?? catalog?.ActiveProfileId;
        selectAfterRefresh = null;
        var items = catalog?.Profiles.Select(profile => new ProfileListItem(profile)).ToList() ?? [];
        ProfileList.ItemsSource = items;
        ProfileList.SelectedItem = items.FirstOrDefault(item => item.Profile.Id == previous) ?? items.FirstOrDefault();
        var empty = items.Count == 0;
        EmptyPanel.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        DetailPanel.Visibility = empty ? Visibility.Collapsed : Visibility.Visible;
        if (controller.CatalogError is { } error)
        {
            SetStatus(error);
        }
        UpdateDetails();
    }

    private void UpdateDetails()
    {
        var profile = Selected;
        DetailGrid.Children.Clear();
        DetailGrid.RowDefinitions.Clear();
        if (profile is null)
        {
            DetailTitle.Text = string.Empty;
            DetailBadge.Text = string.Empty;
            SetButtons(null);
            return;
        }
        DetailTitle.Text = profile.DisplayName;
        DetailBadge.Text = profile.Usable
            ? (profile.Active ? "Active destination — uploads go here." : "Not active.")
              + (profile.Editable ? string.Empty : " Written by hand, so it is read-only here.")
            : $"This profile cannot be used: {DescribeProblem(profile.Problem)}.";
        AddRow("Profile ID", profile.Id);
        if (profile.Usable)
        {
            AddRow("SSH host or alias", profile.Host);
            AddRow("Remote home", profile.RemoteHome);
            AddRow("Upload folder", profile.RemoteDirectory);
            AddRow("Uploads land in", profile.RemoteRoot);
            AddRow("No clipboard image", profile.ShotMode == ShotMode.Full ? "Capture the full screen" : "Capture a region");
            AddRow("Restore clipboard after", profile.RestoreSeconds == 0 ? "Never — keep the path" : $"{profile.RestoreSeconds} seconds");
        }
        if (!profile.Usable || !profile.Editable)
        {
            // Read-only and broken profiles are fixed by editing the file, so show where it is.
            AddRow("Stored in", AppPaths.DisplayPath(System.IO.Path.Combine(controller.Catalog?.ConfigRoot ?? string.Empty, "profiles", $"{profile.Id}.env")));
        }
        SetButtons(profile);
    }

    private void AddRow(string label, string value)
    {
        var row = DetailGrid.RowDefinitions.Count;
        DetailGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        var name = new TextBlock { Text = label, Opacity = 0.7, Margin = new Thickness(0, 4, 12, 4) };
        var text = new TextBox
        {
            Text = value,
            IsReadOnly = true,
            BorderThickness = new Thickness(0),
            Background = System.Windows.Media.Brushes.Transparent,
            Margin = new Thickness(0, 2, 0, 2),
            TextWrapping = TextWrapping.Wrap,
        };
        System.Windows.Automation.AutomationProperties.SetName(text, label);
        Grid.SetRow(name, row);
        Grid.SetRow(text, row);
        Grid.SetColumn(text, 1);
        DetailGrid.Children.Add(name);
        DetailGrid.Children.Add(text);
    }

    private void SetButtons(ProfileInfo? profile)
    {
        var any = profile is not null && !busy;
        UseButton.IsEnabled = any && profile!.Usable && !profile.Active;
        TestButton.IsEnabled = any && profile!.Usable;
        EditButton.IsEnabled = any && profile!.Editable || any && profile!.Problem?.Code == "invalid_field";
        RenameButton.IsEnabled = any;
        DeleteButton.IsEnabled = any;
        DuplicateButton.IsEnabled = any && profile!.Usable;
        AddButton.IsEnabled = !busy;
    }

    private void SetStatus(string message)
    {
        StatusText.Text = message;
        var peer = UIElementAutomationPeer.FromElement(StatusText) ?? UIElementAutomationPeer.CreatePeerForElement(StatusText);
        peer?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged);
    }

    private async Task RunAsync(string working, Func<Task<string>> action)
    {
        busy = true;
        SetButtons(Selected);
        SetStatus(working);
        Cursor = Cursors.AppStarting;
        try
        {
            SetStatus(await action());
        }
        catch (CliException exception)
        {
            SetStatus(ErrorMessages.Describe(exception.Error));
        }
        finally
        {
            Cursor = null;
            busy = false;
            await controller.RefreshCatalogAsync();
            SetButtons(Selected);
        }
    }

    private void ProfileList_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateDetails();

    private void ProfileList_MouseDoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (EditButton.IsEnabled)
        {
            Edit_Click(sender, e);
        }
    }

    private void Add_Click(object sender, RoutedEventArgs e) => _ = AddAsync(null);

    private void Duplicate_Click(object sender, RoutedEventArgs e) => _ = AddAsync(Selected);

    private async Task AddAsync(ProfileInfo? template)
    {
        var existing = controller.Catalog?.Profiles.Select(profile => profile.Id).ToList() ?? [];
        var editor = new ProfileEditorWindow(controller.Service, ProfileEditorMode.Create, template, existing) { Owner = this };
        if (editor.ShowDialog() == true && editor.SavedId is { } id)
        {
            selectAfterRefresh = id;
            await controller.RefreshCatalogAsync();
            SetStatus(editor.SavedMessage ?? $"Added {id}.");
        }
    }

    private async void Edit_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } profile)
        {
            return;
        }
        var editor = new ProfileEditorWindow(controller.Service, ProfileEditorMode.Edit, profile, []) { Owner = this };
        if (editor.ShowDialog() == true)
        {
            selectAfterRefresh = profile.Id;
            await controller.RefreshCatalogAsync();
            SetStatus(editor.SavedMessage ?? $"Saved {profile.DisplayName}.");
        }
    }

    private void Use_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } profile)
        {
            return;
        }
        _ = RunAsync($"Activating {profile.Label}…", async () =>
        {
            await controller.Service.UseProfileAsync(profile.Id);
            return $"Uploads now go to {profile.Label}.";
        });
    }

    private void Test_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } profile)
        {
            return;
        }
        _ = RunAsync($"Connecting to {profile.Host}…", async () =>
        {
            var result = await controller.Service.TestProfileAsync(profile.Id);
            return result.HomeMatches
                ? $"Connection OK. {profile.Host} reports home {result.DetectedHome}, which matches."
                : $"Connected, but {profile.Host} reports home {result.DetectedHome} while this destination uses {result.ConfiguredHome}. Edit the remote home if uploads land in the wrong place.";
        });
    }

    private void Rename_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } profile)
        {
            return;
        }
        var dialog = new InputDialog(
            "Rename profile",
            $"New profile ID for {profile.DisplayName}. IDs use letters, digits, dashes, and underscores.",
            profile.Id,
            value => ProfileRules.IsValidId(value) ? null : "Start with a letter or digit, then use letters, digits, - or _.")
        { Owner = this };
        if (dialog.ShowDialog() != true || dialog.Value == profile.Id)
        {
            return;
        }
        var newId = dialog.Value;
        selectAfterRefresh = newId;
        _ = RunAsync("Renaming…", async () =>
        {
            await controller.Service.RenameProfileAsync(profile.Id, newId);
            return $"Renamed {profile.Id} to {newId}.";
        });
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (Selected is not { } profile || controller.Catalog is not { } catalog)
        {
            return;
        }
        string? switchTo = null;
        if (profile.Usable && profile.Active)
        {
            var others = catalog.Usable.Where(other => other.Id != profile.Id).ToList();
            if (others.Count == 0)
            {
                SetStatus("You cannot delete your only working destination. Add another one first.");
                return;
            }
            var chooser = new ChoiceDialog(
                "Delete active destination",
                $"{profile.Label} is the active destination. Choose where uploads should go instead, then delete it.",
                others.Select(other => (other.Id, $"{other.Label}  ({other.Host})")).ToList(),
                "Delete")
            { Owner = this };
            if (chooser.ShowDialog() != true)
            {
                return;
            }
            switchTo = chooser.SelectedId;
        }
        else if (System.Windows.MessageBox.Show(
                     this,
                     $"Delete the destination \"{profile.DisplayName}\"? This removes its profile file. Uploaded images on the host are not touched.",
                     "Delete destination",
                     MessageBoxButton.OKCancel,
                     MessageBoxImage.Warning,
                     MessageBoxResult.Cancel) != MessageBoxResult.OK)
        {
            return;
        }
        _ = RunAsync("Deleting…", async () =>
        {
            await controller.Service.DeleteProfileAsync(profile.Id, switchTo);
            return $"Deleted {profile.DisplayName}.";
        });
    }
}
