using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Media.Imaging;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;
using WpfClipboard = System.Windows.Clipboard;

namespace SSHImagePaste.Windows.Clipboard;

/// <summary>A copy of clipboard formats that can be put back later.</summary>
public sealed class ClipboardSnapshot
{
    internal ClipboardSnapshot(IReadOnlyList<KeyValuePair<string, object>> entries) => Entries = entries;

    internal IReadOnlyList<KeyValuePair<string, object>> Entries { get; }

    public bool IsEmpty => Entries.Count == 0;
}

/// <summary>
/// Clipboard access for the UI thread. Windows lets only one process open the
/// clipboard at a time, so every call retries briefly before giving up.
/// </summary>
public sealed class ClipboardService
{
    private const int ClipboardBusy = unchecked((int)0x800401D0); // CLIPBRD_E_CANT_OPEN
    private const long MaxSnapshotBytes = 64L * 1024 * 1024;
    private static readonly string[] ImageExtensions = [".png", ".jpg", ".jpeg", ".bmp", ".gif", ".tif", ".tiff", ".webp"];

    public static uint SequenceNumber => NativeMethods.GetClipboardSequenceNumber();

    /// <summary>
    /// Reads an image as PNG bytes. Prefers a PNG stream (keeps transparency),
    /// then a bitmap, then a single copied image file from Explorer.
    /// </summary>
    public byte[]? TryReadImageAsPng()
    {
        return Retry(() =>
        {
            var data = WpfClipboard.GetDataObject();
            if (data is null)
            {
                return null;
            }
            if (data.GetDataPresent("PNG", false) && data.GetData("PNG", false) is MemoryStream pngStream)
            {
                var bytes = pngStream.ToArray();
                if (PngEncoding.IsPng(bytes))
                {
                    return bytes;
                }
            }
            if (WpfClipboard.ContainsImage())
            {
                var image = WpfClipboard.GetImage();
                if (image is not null && image.PixelWidth > 0 && image.PixelHeight > 0)
                {
                    return PngEncoding.Encode(image);
                }
            }
            if (WpfClipboard.ContainsFileDropList())
            {
                var files = WpfClipboard.GetFileDropList();
                if (files.Count == 1 && files[0] is { } path && IsImageFile(path))
                {
                    return PngEncoding.FromFile(path);
                }
            }
            return null;
        });
    }

    public static bool IsImageFile(string path) =>
        ImageExtensions.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase) && File.Exists(path);

    public string? TryGetText() => Retry(() => WpfClipboard.ContainsText() ? WpfClipboard.GetText() : null);

    /// <summary>Copies the formats that can be faithfully restored.</summary>
    public ClipboardSnapshot Snapshot()
    {
        try
        {
            return Retry(() =>
            {
                var entries = new List<KeyValuePair<string, object>>();
                var data = WpfClipboard.GetDataObject();
                if (data is null)
                {
                    return new ClipboardSnapshot(entries);
                }
                long total = 0;
                foreach (var format in data.GetFormats(false))
                {
                    object? value;
                    try
                    {
                        value = data.GetData(format, false);
                    }
                    catch (Exception)
                    {
                        continue;
                    }
                    var copy = CopyValue(value, ref total);
                    if (copy is not null)
                    {
                        entries.Add(new KeyValuePair<string, object>(format, copy));
                    }
                    if (total > MaxSnapshotBytes)
                    {
                        Log.Warn("Clipboard snapshot exceeded the size limit; it will not be restored.");
                        return new ClipboardSnapshot([]);
                    }
                }
                return new ClipboardSnapshot(entries);
            }) ?? new ClipboardSnapshot([]);
        }
        catch (Exception exception)
        {
            Log.Warn($"Clipboard snapshot failed: {exception.Message}");
            return new ClipboardSnapshot([]);
        }
    }

    private static object? CopyValue(object? value, ref long total)
    {
        switch (value)
        {
            case string text:
                total += text.Length * 2L;
                return text;
            case string[] lines:
                total += lines.Sum(line => (long)line.Length * 2);
                return lines.ToArray();
            case MemoryStream stream:
                total += stream.Length;
                return new MemoryStream(stream.ToArray(), writable: false);
            case BitmapSource bitmap:
                total += (long)bitmap.PixelWidth * bitmap.PixelHeight * 4;
                if (!bitmap.IsFrozen && bitmap.CanFreeze)
                {
                    bitmap.Freeze();
                }
                return bitmap;
            default:
                return null;
        }
    }

    /// <summary>
    /// Puts text on the clipboard, kept out of clipboard history and cloud
    /// sync, and returns the resulting clipboard sequence number.
    /// </summary>
    public uint? SetText(string text)
    {
        var data = new DataObject();
        data.SetData(DataFormats.UnicodeText, text);
        data.SetData(DataFormats.Text, text);
        data.SetData("CanIncludeInClipboardHistory", new MemoryStream(BitConverter.GetBytes(0)));
        data.SetData("CanUploadToCloudClipboard", new MemoryStream(BitConverter.GetBytes(0)));
        var ok = Retry(() =>
        {
            WpfClipboard.SetDataObject(data, copy: true);
            return true;
        });
        return ok ? SequenceNumber : null;
    }

    public bool Restore(ClipboardSnapshot snapshot)
    {
        return Retry(() =>
        {
            if (snapshot.IsEmpty)
            {
                WpfClipboard.Clear();
                return true;
            }
            var data = new DataObject();
            foreach (var (format, value) in snapshot.Entries)
            {
                var restored = value is MemoryStream stream ? new MemoryStream(stream.ToArray()) : value;
                try
                {
                    data.SetData(format, restored, false);
                }
                catch (Exception)
                {
                    // Skip formats WPF cannot re-publish.
                }
            }
            WpfClipboard.SetDataObject(data, copy: true);
            return true;
        });
    }

    private static T? Retry<T>(Func<T?> action)
    {
        for (var attempt = 0; ; attempt++)
        {
            try
            {
                return action();
            }
            catch (COMException exception) when (exception.HResult == ClipboardBusy && attempt < 10)
            {
                Thread.Sleep(30 + attempt * 20);
            }
            catch (ExternalException exception) when (attempt < 10)
            {
                Log.Warn($"Clipboard busy: {exception.Message}");
                Thread.Sleep(30 + attempt * 20);
            }
        }
    }
}
