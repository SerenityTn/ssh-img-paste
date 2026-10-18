using System.Drawing;
using SSHImagePaste.Windows.Capture;
using SSHImagePaste.Windows.Cli;
using SSHImagePaste.Windows.Clipboard;
using SSHImagePaste.Windows.Imaging;
using SSHImagePaste.Windows.Platform;

namespace SSHImagePaste.Windows.Upload;

public enum UploadSource
{
    /// <summary>Clipboard image, else the destination's screenshot fallback.</summary>
    Auto,
    Region,
    FullScreen,
}

public enum UploadResultKind
{
    Uploaded,
    Cancelled,
    NoDestination,
    NoImage,
    Busy,
    Failed,
}

public sealed record UploadResult(
    UploadResultKind Kind,
    string Message,
    string? RemotePath = null,
    ProfileInfo? Profile = null,
    bool FromScreenshot = false,
    bool PathCopied = false,
    int RestoreSeconds = 0)
{
    public bool Succeeded => Kind == UploadResultKind.Uploaded;
}

/// <summary>
/// The upload workflow: acquire an image, hand a private PNG to the CLI,
/// then place the remote path on the clipboard and restore the earlier
/// clipboard after the destination's delay if nothing else replaced it.
/// Runs on the UI thread because the clipboard requires an STA thread.
/// </summary>
public sealed class UploadCoordinator(
    SshImagePasteService service,
    ClipboardService clipboard,
    Func<IDisposable> hideWindowsForCapture)
{
    private static readonly TimeSpan HideSettleDelay = TimeSpan.FromMilliseconds(220);
    private readonly ClipboardRestorePolicy restorePolicy = new();

    public bool IsBusy { get; private set; }

    public event EventHandler<string>? Progress;

    public async Task<UploadResult> RunAsync(UploadSource source, CancellationToken cancellationToken = default)
    {
        if (IsBusy)
        {
            return new UploadResult(UploadResultKind.Busy, "An upload is already in progress.");
        }
        IsBusy = true;
        string? stagingPath = null;
        try
        {
            Progress?.Invoke(this, "Checking destination…");
            var catalog = await service.GetCatalogAsync(cancellationToken);
            if (catalog.Active is not { } profile)
            {
                return new UploadResult(UploadResultKind.NoDestination, "Add an SSH destination before uploading.");
            }

            var (png, fromScreenshot, cancelled) = await AcquireAsync(source, profile);
            if (cancelled)
            {
                return new UploadResult(UploadResultKind.Cancelled, "Capture cancelled.", Profile: profile);
            }
            if (png is null)
            {
                return new UploadResult(UploadResultKind.NoImage, "No image found on the clipboard.", Profile: profile);
            }

            stagingPath = StagingFiles.Write(png);
            Progress?.Invoke(this, $"Uploading to {profile.Label}…");
            var remotePath = await service.UploadFileAsync(stagingPath, profile.Id, cancellationToken);
            Log.Info($"Uploaded {png.Length} bytes to {profile.Id}: {remotePath}");

            var copied = PlacePathOnClipboard(remotePath, profile.RestoreSeconds);
            return new UploadResult(
                UploadResultKind.Uploaded,
                copied ? "Remote path copied to the clipboard." : "Uploaded, but the clipboard was busy. Copy the path from the window.",
                remotePath,
                profile,
                fromScreenshot,
                copied,
                profile.RestoreSeconds);
        }
        catch (CliException exception)
        {
            Log.Warn($"Upload failed: {exception.Error.Code} {exception.Error.Reason} {exception.Error.Diagnostic}");
            return new UploadResult(UploadResultKind.Failed, ErrorMessages.Describe(exception.Error));
        }
        catch (OperationCanceledException)
        {
            return new UploadResult(UploadResultKind.Cancelled, "Upload cancelled.");
        }
        catch (Exception exception)
        {
            Log.Error("Upload failed unexpectedly", exception);
            return new UploadResult(UploadResultKind.Failed, $"Upload failed: {exception.Message}");
        }
        finally
        {
            if (stagingPath is not null)
            {
                StagingFiles.Delete(stagingPath);
            }
            IsBusy = false;
        }
    }

    private async Task<(byte[]? Png, bool FromScreenshot, bool Cancelled)> AcquireAsync(UploadSource source, ProfileInfo profile)
    {
        if (source == UploadSource.Auto)
        {
            var fromClipboard = clipboard.TryReadImageAsPng();
            if (fromClipboard is not null)
            {
                return (fromClipboard, false, false);
            }
            source = profile.ShotMode == ShotMode.Full ? UploadSource.FullScreen : UploadSource.Region;
        }

        Bitmap? bitmap;
        using (hideWindowsForCapture())
        {
            await Task.Delay(HideSettleDelay);
            bitmap = source == UploadSource.Region
                ? await RegionSelector.SelectAsync()
                : ScreenCapture.Capture(ScreenCapture.CurrentMonitorBounds);
        }
        if (bitmap is null)
        {
            return (null, true, true);
        }
        using (bitmap)
        {
            Progress?.Invoke(this, "Preparing PNG…");
            var png = await Task.Run(() => PngEncoding.Encode(bitmap));
            return (png, true, false);
        }
    }

    private bool PlacePathOnClipboard(string remotePath, int restoreSeconds)
    {
        var snapshot = restoreSeconds > 0 ? clipboard.Snapshot() : null;
        var sequence = clipboard.SetText(remotePath);
        if (sequence is null)
        {
            return false;
        }
        if (snapshot is null)
        {
            return true;
        }
        var claim = restorePolicy.Begin(remotePath, sequence);
        _ = RestoreLaterAsync(claim, snapshot, TimeSpan.FromSeconds(restoreSeconds));
        return true;
    }

    private async Task RestoreLaterAsync(ClipboardClaim claim, ClipboardSnapshot snapshot, TimeSpan delay)
    {
        try
        {
            await Task.Delay(delay);
            if (!restorePolicy.IsActive(claim))
            {
                return;
            }
            if (restorePolicy.ShouldRestore(claim, clipboard.TryGetText(), ClipboardService.SequenceNumber))
            {
                clipboard.Restore(snapshot);
                Log.Info("Restored the previous clipboard contents.");
            }
            restorePolicy.Complete(claim);
        }
        catch (Exception exception)
        {
            Log.Warn($"Clipboard restore skipped: {exception.Message}");
        }
    }
}
