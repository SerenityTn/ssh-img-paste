using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace SSHImagePaste.Windows.Imaging;

public enum TrayState
{
    Idle,
    Busy,
    Success,
    Error,
}

/// <summary>
/// Vector app and tray icons drawn from assets/AppIcon.svg geometry, so
/// every size stays crisp without a separate rasterizer. Shared with the
/// asset generator that writes the packaged .ico and logo files.
/// </summary>
public static class IconFactory
{
    private static readonly Color Navy = Color.FromRgb(0x17, 0x24, 0x3A);
    private static readonly Color NavyDeep = Color.FromRgb(0x0B, 0x12, 0x20);
    private static readonly Color Cyan = Color.FromRgb(0x55, 0xD7, 0xFF);
    private static readonly Color Blue = Color.FromRgb(0x21, 0x8B, 0xFF);
    private static readonly Color Paper = Color.FromRgb(0xF8, 0xFA, 0xFC);
    private static readonly Color Sky = Color.FromRgb(0xDC, 0xEA, 0xF5);

    private static Brush Freeze(Brush brush)
    {
        brush.Freeze();
        return brush;
    }

    private static Brush Solid(Color color) => Freeze(new SolidColorBrush(color));

    private static Brush Linear(Color from, Color to, Point start, Point end) =>
        Freeze(new LinearGradientBrush(from, to, start, end) { MappingMode = BrushMappingMode.Absolute });

    private static Geometry Rounded(double x, double y, double width, double height, double radius) =>
        new RectangleGeometry(new Rect(x, y, width, height), radius, radius);

    /// <summary>The full-detail icon on a 1024 unit canvas.</summary>
    public static Drawing CreateAppIcon()
    {
        var group = new DrawingGroup();
        var accent = Linear(Cyan, Blue, new Point(300, 290), new Point(720, 730));
        group.Children.Add(new GeometryDrawing(
            Linear(Navy, NavyDeep, new Point(160, 120), new Point(864, 904)),
            null,
            Rounded(52, 52, 920, 920, 220)));
        group.Children.Add(new GeometryDrawing(
            null,
            new Pen(Freeze(new SolidColorBrush(Color.FromArgb(0xA6, 0x21, 0x32, 0x50))), 52) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round },
            Geometry.Parse("M176 736C286 632 370 714 478 602C588 488 668 520 848 332")));
        group.Children.Add(Shadowed(Rounded(176, 224, 568, 474, 92)));
        group.Children.Add(new GeometryDrawing(Solid(Paper), null, Rounded(176, 224, 568, 474, 92)));
        group.Children.Add(new GeometryDrawing(Solid(Sky), null, Rounded(224, 272, 472, 378, 54)));
        group.Children.Add(new GeometryDrawing(Solid(Cyan), null, new EllipseGeometry(new Point(331, 374), 48, 48)));
        group.Children.Add(new GeometryDrawing(accent, null, Geometry.Parse(
            "M242 614L382 464C397 448 423 448 438 465L509 543L570 482C585 467 609 467 624 482L678 538V632H242V614Z")));
        group.Children.Add(new GeometryDrawing(
            Freeze(new SolidColorBrush(Color.FromArgb(0xD1, 0x16, 0x75, 0xDB))),
            null,
            Geometry.Parse("M624 482L678 538V632H510L570 482C585 467 609 467 624 482Z")));
        group.Children.Add(Shadowed(Rounded(570, 532, 286, 246, 68)));
        group.Children.Add(new GeometryDrawing(Solid(Color.FromRgb(0x12, 0x1D, 0x30)), new Pen(Solid(Cyan), 14), Rounded(570, 532, 286, 246, 68)));
        group.Children.Add(new GeometryDrawing(Solid(Cyan), null, new EllipseGeometry(new Point(642, 617), 16, 16)));
        group.Children.Add(new GeometryDrawing(Solid(Color.FromRgb(0xE8, 0xF5, 0xFF)), null, Rounded(683, 602, 105, 30, 15)));
        group.Children.Add(new GeometryDrawing(Solid(Blue), null, new EllipseGeometry(new Point(642, 694), 16, 16)));
        group.Children.Add(new GeometryDrawing(Solid(Color.FromRgb(0xA9, 0xC8, 0xE6)), null, Rounded(683, 679, 105, 30, 15)));
        group.Children.Add(UploadBadge(695, 276, 120, accent));
        group.Freeze();
        return group;
    }

    private static Drawing Shadowed(Geometry geometry)
    {
        var shadow = geometry.Clone();
        shadow.Transform = new TranslateTransform(0, 26);
        return new GeometryDrawing(Freeze(new SolidColorBrush(Color.FromArgb(0x55, 0x02, 0x06, 0x17))), null, shadow);
    }

    private static Drawing UploadBadge(double x, double y, double radius, Brush fill)
    {
        var group = new DrawingGroup();
        group.Children.Add(new GeometryDrawing(fill, new Pen(Solid(Paper), radius * 0.15), new EllipseGeometry(new Point(x, y), radius, radius)));
        var arrow = Geometry.Parse("M0 62V-62M0-62L-45-17M0-62L45-17").Clone();
        var scale = radius / 120.0;
        arrow.Transform = new TransformGroup { Children = { new ScaleTransform(scale, scale), new TranslateTransform(x, y) } };
        group.Children.Add(new GeometryDrawing(null, new Pen(Brushes.White, 30 * scale) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round }, arrow));
        return group;
    }

    /// <summary>
    /// A simplified glyph for 16–32 px tray slots: photo frame plus upload
    /// badge, with a state badge in the lower right.
    /// </summary>
    public static Drawing CreateTrayIcon(TrayState state)
    {
        var group = new DrawingGroup();
        group.Children.Add(new GeometryDrawing(Linear(Navy, NavyDeep, new Point(0, 0), new Point(1024, 1024)), null, Rounded(24, 24, 976, 976, 230)));
        group.Children.Add(new GeometryDrawing(Solid(Paper), null, Rounded(150, 250, 620, 520, 110)));
        group.Children.Add(new GeometryDrawing(Solid(Sky), null, Rounded(210, 310, 500, 400, 60)));
        group.Children.Add(new GeometryDrawing(Linear(Cyan, Blue, new Point(250, 400), new Point(700, 700)), null, Geometry.Parse(
            "M230 690L390 520C405 504 431 504 446 521L520 600L585 540C600 525 624 525 639 540L690 595V690Z")));
        var badgeColor = state switch
        {
            TrayState.Busy => Color.FromRgb(0xF5, 0x9E, 0x0B),
            TrayState.Success => Color.FromRgb(0x22, 0xC5, 0x5E),
            TrayState.Error => Color.FromRgb(0xEF, 0x44, 0x44),
            _ => Blue,
        };
        if (state == TrayState.Idle)
        {
            group.Children.Add(UploadBadge(740, 300, 230, Linear(Cyan, Blue, new Point(510, 70), new Point(970, 530))));
        }
        else
        {
            group.Children.Add(new GeometryDrawing(Solid(badgeColor), new Pen(Solid(Paper), 44), new EllipseGeometry(new Point(740, 740), 250, 250)));
            var glyph = state switch
            {
                TrayState.Busy => "M640 740H840M740 640L840 740L740 840",
                TrayState.Success => "M630 745L715 830L860 660",
                _ => "M740 630V770M740 850V852",
            };
            group.Children.Add(new GeometryDrawing(
                null,
                new Pen(Brushes.White, 70) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round },
                Geometry.Parse(glyph)));
        }
        group.Freeze();
        return group;
    }

    /// <summary>Renders a 1024-unit drawing to a square premultiplied bitmap.</summary>
    public static BitmapSource Render(Drawing drawing, int size)
    {
        var visual = new DrawingVisual();
        using (var context = visual.RenderOpen())
        {
            context.PushTransform(new ScaleTransform(size / 1024.0, size / 1024.0));
            context.DrawDrawing(drawing);
            context.Pop();
        }
        RenderOptions.SetBitmapScalingMode(visual, BitmapScalingMode.HighQuality);
        var bitmap = new RenderTargetBitmap(size, size, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    public static byte[] RenderPng(Drawing drawing, int size)
    {
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(Render(drawing, size)));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    /// <summary>
    /// Builds a multi-size .ico. Sizes up to 64 px use 32-bit DIB entries for
    /// maximum shell compatibility; larger sizes use PNG entries.
    /// </summary>
    public static byte[] CreateIco(Drawing drawing, IReadOnlyList<int> sizes)
    {
        var images = sizes.Select(size => (size, data: size > 64 ? RenderPng(drawing, size) : DibEntry(Render(drawing, size)))).ToList();
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write((ushort)0);
        writer.Write((ushort)1);
        writer.Write((ushort)images.Count);
        var offset = 6 + 16 * images.Count;
        foreach (var (size, data) in images)
        {
            writer.Write((byte)(size >= 256 ? 0 : size));
            writer.Write((byte)(size >= 256 ? 0 : size));
            writer.Write((byte)0);
            writer.Write((byte)0);
            writer.Write((ushort)1);
            writer.Write((ushort)32);
            writer.Write(data.Length);
            writer.Write(offset);
            offset += data.Length;
        }
        foreach (var (_, data) in images)
        {
            writer.Write(data);
        }
        writer.Flush();
        return stream.ToArray();
    }

    private static byte[] DibEntry(BitmapSource bitmap)
    {
        var width = bitmap.PixelWidth;
        var height = bitmap.PixelHeight;
        var stride = width * 4;
        var pixels = new byte[stride * height];
        bitmap.CopyPixels(pixels, stride, 0);
        // Premultiplied to straight alpha, as icon DIBs expect.
        for (var index = 0; index < pixels.Length; index += 4)
        {
            var alpha = pixels[index + 3];
            if (alpha is > 0 and < 255)
            {
                for (var channel = 0; channel < 3; channel++)
                {
                    pixels[index + channel] = (byte)Math.Min(255, pixels[index + channel] * 255 / alpha);
                }
            }
        }
        var maskStride = (width + 31) / 32 * 4;
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write(40);
        writer.Write(width);
        writer.Write(height * 2);
        writer.Write((ushort)1);
        writer.Write((ushort)32);
        writer.Write(0);
        writer.Write(pixels.Length + maskStride * height);
        writer.Write(0);
        writer.Write(0);
        writer.Write(0);
        writer.Write(0);
        for (var row = height - 1; row >= 0; row--)
        {
            writer.Write(pixels, row * stride, stride);
        }
        writer.Write(new byte[maskStride * height]);
        writer.Flush();
        return stream.ToArray();
    }

    public static System.Drawing.Icon CreateTrayIconHandle(TrayState state, int size)
    {
        using var stream = new MemoryStream(CreateIco(CreateTrayIcon(state), [size]));
        return new System.Drawing.Icon(stream, size, size);
    }
}
