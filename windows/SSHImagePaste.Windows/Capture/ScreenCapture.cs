using System.Drawing;
using Forms = System.Windows.Forms;

namespace SSHImagePaste.Windows.Capture;

/// <summary>
/// GDI screen capture in physical pixels. The process is per-monitor DPI
/// aware (see app.manifest), so screen bounds are not scaled.
/// </summary>
public static class ScreenCapture
{
    public static Rectangle VirtualScreen => Forms.SystemInformation.VirtualScreen;

    public static Bitmap Capture(Rectangle bounds)
    {
        if (bounds.Width <= 0 || bounds.Height <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(bounds), "Capture bounds are empty.");
        }
        var bitmap = new Bitmap(bounds.Width, bounds.Height, System.Drawing.Imaging.PixelFormat.Format32bppRgb);
        try
        {
            using var graphics = Graphics.FromImage(bitmap);
            graphics.CopyFromScreen(
                bounds.Location,
                Point.Empty,
                bounds.Size,
                CopyPixelOperation.SourceCopy | CopyPixelOperation.CaptureBlt);
            return bitmap;
        }
        catch
        {
            bitmap.Dispose();
            throw;
        }
    }

    /// <summary>The monitor under the mouse pointer.</summary>
    public static Rectangle CurrentMonitorBounds => Forms.Screen.FromPoint(Forms.Cursor.Position).Bounds;

    public static Bitmap Crop(Bitmap source, Rectangle area)
    {
        var clipped = Rectangle.Intersect(area, new Rectangle(Point.Empty, source.Size));
        if (clipped.Width <= 0 || clipped.Height <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(area), "Selection is outside the captured image.");
        }
        return source.Clone(clipped, source.PixelFormat);
    }
}
