using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace SSHImagePaste.Windows;

/// <summary>Builds the small dialogs in code so they share one layout.</summary>
internal static class DialogLayout
{
    public static (StackPanel Body, Button Ok) Create(Window window, string title, string prompt, string okText)
    {
        window.Title = title;
        window.Width = 440;
        window.SizeToContent = SizeToContent.Height;
        window.ResizeMode = ResizeMode.NoResize;
        window.WindowStartupLocation = WindowStartupLocation.CenterOwner;
        window.ShowInTaskbar = false;
        var root = new StackPanel { Margin = new Thickness(24, 18, 24, 20) };
        root.Children.Add(new TextBlock { Text = title, FontSize = 18, FontWeight = FontWeights.SemiBold });
        root.Children.Add(new TextBlock { Text = prompt, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 8, 0, 12), Opacity = 0.85 });
        var body = new StackPanel();
        root.Children.Add(body);
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 18, 0, 0) };
        var ok = new Button { Content = okText, MinWidth = 96, IsDefault = true };
        ok.SetResourceReference(FrameworkElement.StyleProperty, "AccentButtonStyle");
        var cancel = new Button { Content = "Cancel", MinWidth = 96, Margin = new Thickness(8, 0, 0, 0), IsCancel = true };
        buttons.Children.Add(ok);
        buttons.Children.Add(cancel);
        root.Children.Add(buttons);
        window.Content = root;
        return (body, ok);
    }
}

public sealed class InputDialog : Window
{
    private readonly TextBox input;
    private readonly TextBlock error;
    private readonly Func<string, string?> validate;

    public InputDialog(string title, string prompt, string initial, Func<string, string?> validate)
    {
        this.validate = validate;
        var (body, ok) = DialogLayout.Create(this, title, prompt, "OK");
        input = new TextBox { Text = initial };
        System.Windows.Automation.AutomationProperties.SetName(input, title);
        error = new TextBlock { Foreground = new SolidColorBrush(Color.FromRgb(0xD1, 0x34, 0x38)), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 4, 0, 0) };
        body.Children.Add(input);
        body.Children.Add(error);
        input.TextChanged += (_, _) => error.Text = validate(input.Text.Trim()) ?? string.Empty;
        ok.Click += (_, _) =>
        {
            var message = validate(input.Text.Trim());
            if (message is not null)
            {
                error.Text = message;
                return;
            }
            DialogResult = true;
        };
        Loaded += (_, _) =>
        {
            input.Focus();
            input.SelectAll();
        };
    }

    public string Value => input.Text.Trim();
}

public sealed class ChoiceDialog : Window
{
    private readonly ComboBox choices;

    public ChoiceDialog(string title, string prompt, IReadOnlyList<(string Id, string Label)> options, string okText)
    {
        var (body, ok) = DialogLayout.Create(this, title, prompt, okText);
        choices = new ComboBox
        {
            ItemsSource = options.Select(option => new ComboBoxItem { Content = option.Label, Tag = option.Id }).ToList(),
            SelectedIndex = 0,
        };
        System.Windows.Automation.AutomationProperties.SetName(choices, "Replacement destination");
        body.Children.Add(choices);
        ok.Click += (_, _) => DialogResult = true;
    }

    public string? SelectedId => (choices.SelectedItem as ComboBoxItem)?.Tag as string;
}
