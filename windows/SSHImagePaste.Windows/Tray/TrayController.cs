using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;
using Forms = System.Windows.Forms;

namespace SSHImagePaste.Windows.Tray;

public enum NotificationKind
{
    Info,
    Success,
    Error,
}

/// <summary>Notification-area icon, menu, state badges, and toasts.</summary>
public sealed class TrayController : IDisposable
{
    private static readonly TimeSpan TransientStateDuration = TimeSpan.FromSeconds(3);
    private readonly Forms.NotifyIcon icon;
    private readonly Forms.ContextMenuStrip menu;
    private readonly Forms.ToolStripMenuItem destinations;
    private readonly Dictionary<DesktopAction, Forms.ToolStripMenuItem> actionItems = [];
    private readonly Dictionary<TrayState, System.Drawing.Icon> icons = [];
    private readonly System.Windows.Threading.DispatcherTimer revertTimer;
    private ProfileCatalog? catalog;
    private bool busy;

    public TrayController()
    {
        menu = new Forms.ContextMenuStrip { ShowImageMargin = false, AccessibleName = "SSH Image Paste menu" };
        destinations = new Forms.ToolStripMenuItem(TrayMenuModel.DestinationMenuLabel);
        foreach (var action in TrayMenuModel.PrimaryActions)
        {
            if (TrayMenuModel.StartsGroup(action))
            {
                menu.Items.Add(new Forms.ToolStripSeparator());
            }
            if (action == DesktopAction.ManageProfiles)
            {
                menu.Items.Add(destinations);
            }
            var item = new Forms.ToolStripMenuItem(TrayMenuModel.Label(action));
            item.Click += (_, _) => ActionRequested?.Invoke(this, action);
            if (action == DesktopAction.OpenWindow)
            {
                item.Font = new System.Drawing.Font(item.Font, System.Drawing.FontStyle.Bold);
            }
            actionItems[action] = item;
            menu.Items.Add(item);
        }
        menu.Opening += (_, _) => MenuOpening?.Invoke(this, EventArgs.Empty);

        icon = new Forms.NotifyIcon
        {
            Text = AppPaths.ProductName,
            Icon = IconFor(TrayState.Idle),
            ContextMenuStrip = menu,
            Visible = true,
        };
        icon.DoubleClick += (_, _) => ActionRequested?.Invoke(this, TrayInteraction.ActionForDoubleClick());
        icon.BalloonTipClicked += (_, _) => ActionRequested?.Invoke(this, DesktopAction.OpenWindow);

        revertTimer = new System.Windows.Threading.DispatcherTimer { Interval = TransientStateDuration };
        revertTimer.Tick += (_, _) =>
        {
            revertTimer.Stop();
            if (!busy)
            {
                ApplyState(TrayState.Idle, IdleTooltip());
            }
        };
        UpdateCatalog(null);
    }

    public event EventHandler<DesktopAction>? ActionRequested;

    public event EventHandler<string>? DestinationRequested;

    public event EventHandler? MenuOpening;

    public void UpdateCatalog(ProfileCatalog? updated)
    {
        catalog = updated;
        destinations.DropDownItems.Clear();
        var usable = updated?.Usable.ToList() ?? [];
        if (usable.Count == 0)
        {
            destinations.DropDownItems.Add(new Forms.ToolStripMenuItem("No destinations yet") { Enabled = false });
        }
        foreach (var profile in usable)
        {
            var item = new Forms.ToolStripMenuItem($"{profile.Label}  ({profile.Host})")
            {
                Checked = profile.Id == updated?.ActiveProfileId,
                CheckOnClick = false,
            };
            var id = profile.Id;
            item.Click += (_, _) => DestinationRequested?.Invoke(this, id);
            destinations.DropDownItems.Add(item);
        }
        var hasDestination = updated?.Active is not null;
        foreach (var (action, item) in actionItems)
        {
            item.Enabled = !TrayMenuModel.RequiresDestination(action) || (hasDestination && !(busy && action != DesktopAction.UploadedImages));
        }
        if (!busy && !revertTimer.IsEnabled)
        {
            icon.Text = Truncate(IdleTooltip());
        }
    }

    public void SetBusy(string message)
    {
        busy = true;
        revertTimer.Stop();
        ApplyState(TrayState.Busy, message);
        UpdateCatalog(catalog);
    }

    public void SetResult(bool succeeded, string message)
    {
        busy = false;
        ApplyState(succeeded ? TrayState.Success : TrayState.Error, message);
        revertTimer.Stop();
        revertTimer.Start();
        UpdateCatalog(catalog);
    }

    public void SetIdle()
    {
        busy = false;
        revertTimer.Stop();
        ApplyState(TrayState.Idle, IdleTooltip());
        UpdateCatalog(catalog);
    }

    public void Notify(string title, string text, NotificationKind kind)
    {
        icon.ShowBalloonTip(
            5000,
            title,
            string.IsNullOrWhiteSpace(text) ? " " : text,
            kind switch
            {
                NotificationKind.Error => Forms.ToolTipIcon.Error,
                NotificationKind.Success => Forms.ToolTipIcon.Info,
                _ => Forms.ToolTipIcon.None,
            });
    }

    private string IdleTooltip() =>
        catalog?.Active is { } active ? $"{AppPaths.ProductName} — {active.Label}" : $"{AppPaths.ProductName} — no destination";

    private void ApplyState(TrayState state, string tooltip)
    {
        icon.Icon = IconFor(state);
        icon.Text = Truncate(tooltip);
    }

    private System.Drawing.Icon IconFor(TrayState state)
    {
        if (!icons.TryGetValue(state, out var value))
        {
            var size = Math.Max(16, Forms.SystemInformation.SmallIconSize.Width);
            value = IconFactory.CreateTrayIconHandle(state, size);
            icons[state] = value;
        }
        return value;
    }

    /// <summary>NotifyIcon tooltips are limited to 127 characters.</summary>
    private static string Truncate(string text) => text.Length <= 127 ? text : text[..126] + "…";

    public void Dispose()
    {
        revertTimer.Stop();
        icon.Visible = false;
        icon.Dispose();
        menu.Dispose();
        foreach (var value in icons.Values)
        {
            value.Dispose();
        }
        icons.Clear();
    }
}
