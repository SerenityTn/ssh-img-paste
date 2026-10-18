using System.Drawing;
using System.Drawing.Drawing2D;
using Forms = System.Windows.Forms;

namespace SSHImagePaste.Windows.Capture;

/// <summary>Pure selection math, kept separate for contract tests.</summary>
public static class SelectionGeometry
{
    public const int MinimumSize = 3;

    public static Rectangle Normalize(Point anchor, Point current)
    {
        var left = Math.Min(anchor.X, current.X);
        var top = Math.Min(anchor.Y, current.Y);
        return new Rectangle(left, top, Math.Abs(current.X - anchor.X) + 1, Math.Abs(current.Y - anchor.Y) + 1);
    }

    public static bool IsUsable(Rectangle selection) =>
        selection.Width >= MinimumSize && selection.Height >= MinimumSize;

    /// <summary>Maps a global screen rectangle into frozen-capture coordinates.</summary>
    public static Rectangle ToCapture(Rectangle selection, Rectangle virtualScreen) =>
        Rectangle.Intersect(
            new Rectangle(selection.X - virtualScreen.X, selection.Y - virtualScreen.Y, selection.Width, selection.Height),
            new Rectangle(Point.Empty, virtualScreen.Size));
}

/// <summary>
/// Freeze-frame region picker: the desktop is captured first, then one
/// borderless overlay per monitor shows that frozen image dimmed while the
/// user drags. Esc or right-click cancels.
/// </summary>
public sealed class RegionSelector : IDisposable
{
    private readonly Bitmap frozen;
    private readonly Rectangle virtualScreen;
    private readonly List<OverlayForm> overlays = [];
    private readonly TaskCompletionSource<Rectangle?> completion = new();
    private Point? anchor;
    private Point current;

    private RegionSelector(Bitmap frozen, Rectangle virtualScreen)
    {
        this.frozen = frozen;
        this.virtualScreen = virtualScreen;
    }

    /// <summary>Returns the selected region cropped from the frozen frame, or null when cancelled.</summary>
    public static async Task<Bitmap?> SelectAsync()
    {
        var virtualScreen = ScreenCapture.VirtualScreen;
        var frozen = ScreenCapture.Capture(virtualScreen);
        using var selector = new RegionSelector(frozen, virtualScreen);
        var selection = await selector.RunAsync();
        if (selection is not { } area)
        {
            return null;
        }
        var captureArea = SelectionGeometry.ToCapture(area, virtualScreen);
        return captureArea.Width > 0 && captureArea.Height > 0 ? ScreenCapture.Crop(frozen, captureArea) : null;
    }

    private Task<Rectangle?> RunAsync()
    {
        foreach (var screen in Forms.Screen.AllScreens)
        {
            var overlay = new OverlayForm(this, screen.Bounds);
            overlays.Add(overlay);
            overlay.Show();
        }
        var cursorScreen = Forms.Screen.FromPoint(Forms.Cursor.Position).Bounds;
        var focus = overlays.FirstOrDefault(overlay => overlay.ScreenBounds == cursorScreen) ?? overlays.FirstOrDefault();
        if (focus is not null)
        {
            focus.Activate();
            Platform.NativeMethods.SetForegroundWindow(focus.Handle);
            focus.Focus();
        }
        return completion.Task;
    }

    private Rectangle? Selection => anchor is { } start ? SelectionGeometry.Normalize(start, current) : null;

    private void Begin(Point screenPoint)
    {
        anchor = screenPoint;
        current = screenPoint;
        Invalidate();
    }

    private void Move(Point screenPoint)
    {
        current = screenPoint;
        if (anchor is not null)
        {
            Invalidate();
        }
    }

    private void End(Point screenPoint)
    {
        if (anchor is null)
        {
            return;
        }
        current = screenPoint;
        var selection = Selection!.Value;
        if (!SelectionGeometry.IsUsable(selection))
        {
            anchor = null;
            Invalidate();
            return;
        }
        Finish(selection);
    }

    private void Finish(Rectangle? result)
    {
        if (completion.TrySetResult(result))
        {
            foreach (var overlay in overlays)
            {
                overlay.Hide();
            }
        }
    }

    private void Invalidate()
    {
        foreach (var overlay in overlays)
        {
            overlay.Invalidate();
        }
    }

    public void Dispose()
    {
        completion.TrySetResult(null);
        foreach (var overlay in overlays)
        {
            overlay.Close();
            overlay.Dispose();
        }
        overlays.Clear();
        frozen.Dispose();
    }

    private sealed class OverlayForm : Forms.Form
    {
        private static readonly Color Dim = Color.FromArgb(120, 0, 0, 0);
        private static readonly Color Accent = Color.FromArgb(255, 33, 139, 255);
        private readonly RegionSelector owner;

        public OverlayForm(RegionSelector owner, Rectangle screenBounds)
        {
            this.owner = owner;
            ScreenBounds = screenBounds;
            FormBorderStyle = Forms.FormBorderStyle.None;
            StartPosition = Forms.FormStartPosition.Manual;
            AutoScaleMode = Forms.AutoScaleMode.None;
            Bounds = screenBounds;
            TopMost = true;
            ShowInTaskbar = false;
            KeyPreview = true;
            DoubleBuffered = true;
            Cursor = Forms.Cursors.Cross;
            Text = "Select a region — drag to capture, Esc to cancel";
            AccessibleName = "Region capture overlay";
            AccessibleDescription = "Drag to select the area to upload. Press Escape to cancel.";
            SetStyle(Forms.ControlStyles.AllPaintingInWmPaint | Forms.ControlStyles.UserPaint | Forms.ControlStyles.OptimizedDoubleBuffer, true);
        }

        public Rectangle ScreenBounds { get; }

        protected override bool ShowWithoutActivation => false;

        protected override void OnShown(EventArgs e)
        {
            base.OnShown(e);
            // Mixed-DPI moves can resize the window; pin it back to the monitor.
            Bounds = ScreenBounds;
        }

        protected override void OnDpiChanged(Forms.DpiChangedEventArgs e)
        {
            e.Cancel = true;
            Bounds = ScreenBounds;
        }

        protected override void OnMouseDown(Forms.MouseEventArgs e)
        {
            base.OnMouseDown(e);
            if (e.Button == Forms.MouseButtons.Right)
            {
                owner.Finish(null);
                return;
            }
            if (e.Button == Forms.MouseButtons.Left)
            {
                Capture = true;
                owner.Begin(Forms.Cursor.Position);
            }
        }

        protected override void OnMouseMove(Forms.MouseEventArgs e)
        {
            base.OnMouseMove(e);
            owner.Move(Forms.Cursor.Position);
        }

        protected override void OnMouseUp(Forms.MouseEventArgs e)
        {
            base.OnMouseUp(e);
            if (e.Button == Forms.MouseButtons.Left)
            {
                Capture = false;
                owner.End(Forms.Cursor.Position);
            }
        }

        protected override void OnKeyDown(Forms.KeyEventArgs e)
        {
            base.OnKeyDown(e);
            if (e.KeyCode == Forms.Keys.Escape)
            {
                owner.Finish(null);
            }
        }

        protected override bool ProcessCmdKey(ref Forms.Message msg, Forms.Keys keyData)
        {
            if (keyData == Forms.Keys.Escape)
            {
                owner.Finish(null);
                return true;
            }
            return base.ProcessCmdKey(ref msg, keyData);
        }

        protected override void OnPaintBackground(Forms.PaintEventArgs e)
        {
            // Everything is painted in OnPaint to avoid flicker.
        }

        protected override void OnPaint(Forms.PaintEventArgs e)
        {
            var graphics = e.Graphics;
            var local = new Rectangle(Point.Empty, ScreenBounds.Size);
            var sourceOrigin = new Point(ScreenBounds.X - owner.virtualScreen.X, ScreenBounds.Y - owner.virtualScreen.Y);
            graphics.DrawImage(owner.frozen, local, new Rectangle(sourceOrigin, ScreenBounds.Size), GraphicsUnit.Pixel);

            using var dim = new SolidBrush(Dim);
            if (owner.Selection is not { } selection)
            {
                graphics.FillRectangle(dim, local);
                DrawHint(graphics, local);
                return;
            }

            var selected = new Rectangle(selection.X - ScreenBounds.X, selection.Y - ScreenBounds.Y, selection.Width, selection.Height);
            using (var outside = new Region(local))
            {
                outside.Exclude(selected);
                graphics.FillRegion(dim, outside);
            }
            if (!Rectangle.Intersect(selected, local).IsEmpty)
            {
                using var pen = new Pen(Accent, 2) { Alignment = PenAlignment.Inset };
                graphics.DrawRectangle(pen, selected);
                DrawSizeLabel(graphics, selected, selection.Size, local);
            }
        }

        private void DrawHint(Graphics graphics, Rectangle local)
        {
            if (ScreenBounds != Forms.Screen.FromPoint(Forms.Cursor.Position).Bounds)
            {
                return;
            }
            const string hint = "Drag to select an area  ·  Esc to cancel";
            using var font = new Font("Segoe UI", 11f, FontStyle.Regular, GraphicsUnit.Point);
            var size = graphics.MeasureString(hint, font);
            var padding = 12 * DeviceDpi / 96;
            var box = new RectangleF(
                (local.Width - size.Width) / 2 - padding,
                40 * DeviceDpi / 96f,
                size.Width + padding * 2,
                size.Height + padding);
            using var background = new SolidBrush(Color.FromArgb(220, 18, 29, 48));
            using var path = RoundedRectangle(box, 8 * DeviceDpi / 96f);
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.FillPath(background, path);
            graphics.DrawString(hint, font, Brushes.White, box.X + padding, box.Y + padding / 2f);
        }

        private void DrawSizeLabel(Graphics graphics, Rectangle selected, Size size, Rectangle local)
        {
            var text = $"{size.Width} × {size.Height}";
            using var font = new Font("Segoe UI", 9f, FontStyle.Regular, GraphicsUnit.Point);
            var measured = graphics.MeasureString(text, font);
            var x = Math.Clamp(selected.X, 0, Math.Max(0, local.Width - (int)measured.Width - 8));
            var y = selected.Bottom + 6 + measured.Height > local.Height ? selected.Y - measured.Height - 6 : selected.Bottom + 6;
            y = Math.Clamp(y, 0, Math.Max(0, local.Height - measured.Height));
            using var background = new SolidBrush(Color.FromArgb(220, 18, 29, 48));
            graphics.FillRectangle(background, x, y, measured.Width + 8, measured.Height);
            graphics.DrawString(text, font, Brushes.White, x + 4, y);
        }

        private static GraphicsPath RoundedRectangle(RectangleF bounds, float radius)
        {
            var path = new GraphicsPath();
            var diameter = radius * 2;
            path.AddArc(bounds.X, bounds.Y, diameter, diameter, 180, 90);
            path.AddArc(bounds.Right - diameter, bounds.Y, diameter, diameter, 270, 90);
            path.AddArc(bounds.Right - diameter, bounds.Bottom - diameter, diameter, diameter, 0, 90);
            path.AddArc(bounds.X, bounds.Bottom - diameter, diameter, diameter, 90, 90);
            path.CloseFigure();
            return path;
        }
    }
}
