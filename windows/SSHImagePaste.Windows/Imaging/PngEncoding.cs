using System.Drawing;
using System.Drawing.Imaging;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace SSHImagePaste.Windows.Imaging;

/// <summary>PNG normalization through WIC (WPF imaging).</summary>
public static class PngEncoding
{
    private static readonly byte[] Signature = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    /// <summary>Hard ceiling on decoded pixels, about 32 4K screens.</summary>
    public const long MaxPixels = 268_435_456;

    public static bool IsPng(ReadOnlySpan<byte> bytes) => bytes.Length >= Signature.Length && bytes[..Signature.Length].SequenceEqual(Signature);

    public static byte[] Encode(BitmapSource source)
    {
        if ((long)source.PixelWidth * source.PixelHeight > MaxPixels)
        {
            throw new InvalidOperationException("The image is too large to upload.");
        }
        // Clipboard DIBs often arrive as Bgr32 with an undefined alpha byte;
        // normalizing avoids a fully transparent PNG.
        BitmapSource normalized = source.Format == PixelFormats.Bgr32 || source.Format == PixelFormats.Bgra32 || source.Format == PixelFormats.Pbgra32
            ? source
            : new FormatConvertedBitmap(source, PixelFormats.Bgra32, null, 0);
        if (normalized.Format == PixelFormats.Bgr32 || HasEmptyAlpha(normalized))
        {
            normalized = new FormatConvertedBitmap(normalized, PixelFormats.Bgr24, null, 0);
        }
        var encoder = new PngBitmapEncoder { Interlace = PngInterlaceOption.Off };
        encoder.Frames.Add(BitmapFrame.Create(normalized));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    /// <summary>
    /// True when every pixel has zero alpha, which clipboard DIBs produce when
    /// the alpha byte is padding rather than transparency.
    /// </summary>
    private static bool HasEmptyAlpha(BitmapSource source)
    {
        if (source.Format != PixelFormats.Bgra32 && source.Format != PixelFormats.Pbgra32)
        {
            return false;
        }
        var stride = source.PixelWidth * 4;
        var row = new byte[stride];
        for (var y = 0; y < source.PixelHeight; y++)
        {
            source.CopyPixels(new Int32Rect(0, y, source.PixelWidth, 1), row, stride, 0);
            for (var x = 3; x < row.Length; x += 4)
            {
                if (row[x] != 0)
                {
                    return false;
                }
            }
        }
        return true;
    }

    public static byte[] FromFile(string path)
    {
        using var stream = File.OpenRead(path);
        var decoder = BitmapDecoder.Create(stream, BitmapCreateOptions.PreservePixelFormat | BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.OnLoad);
        return Encode(decoder.Frames[0]);
    }

    /// <summary>Encodes a GDI capture without an intermediate HBITMAP.</summary>
    public static byte[] Encode(Bitmap bitmap)
    {
        var bounds = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
        var data = bitmap.LockBits(bounds, ImageLockMode.ReadOnly, System.Drawing.Imaging.PixelFormat.Format32bppRgb);
        try
        {
            var source = BitmapSource.Create(
                bitmap.Width,
                bitmap.Height,
                96,
                96,
                PixelFormats.Bgr32,
                null,
                data.Scan0,
                data.Stride * bitmap.Height,
                data.Stride);
            return Encode(source);
        }
        finally
        {
            bitmap.UnlockBits(data);
        }
    }

    public static (int Width, int Height) Dimensions(byte[] png)
    {
        using var stream = new MemoryStream(png);
        var frame = BitmapDecoder.Create(stream, BitmapCreateOptions.DelayCreation, BitmapCacheOption.None).Frames[0];
        return (frame.PixelWidth, frame.PixelHeight);
    }

    public static Int32Rect FullRect(BitmapSource source) => new(0, 0, source.PixelWidth, source.PixelHeight);
}
