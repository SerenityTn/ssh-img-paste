using System.Runtime.InteropServices;

namespace SSHImagePaste.Windows.Platform;

internal static partial class NativeMethods
{
    public const int AppModelErrorNoPackage = 15700;
    public const int ErrorInsufficientBuffer = 122;

    /// <summary>Increments whenever any process changes the clipboard.</summary>
    [LibraryImport("user32.dll")]
    public static partial uint GetClipboardSequenceNumber();

    [LibraryImport("kernel32.dll", StringMarshalling = StringMarshalling.Utf16)]
    public static partial int GetCurrentPackageFullName(ref int packageFullNameLength, char[]? packageFullName);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetForegroundWindow(IntPtr hWnd);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool DestroyIcon(IntPtr hIcon);
}
