namespace SSHImagePaste.Windows.Clipboard;

/// <summary>One "path is on the clipboard" ownership claim.</summary>
public sealed record ClipboardClaim(long Generation, string ExpectedText, uint? SequenceNumber);

/// <summary>
/// Port of the core <c>ClipboardCoordinator</c>: restore the earlier clipboard
/// only while this app still owns it. A newer upload supersedes older timers,
/// and any change by another app (seen as a new clipboard sequence number or
/// different text) cancels restoration.
/// </summary>
public sealed class ClipboardRestorePolicy
{
    private long nextGeneration;
    private long? activeGeneration;

    public ClipboardClaim Begin(string expectedText, uint? sequenceNumber)
    {
        nextGeneration = checked(nextGeneration + 1);
        activeGeneration = nextGeneration;
        return new ClipboardClaim(nextGeneration, expectedText, sequenceNumber);
    }

    public bool ShouldRestore(ClipboardClaim claim, string? currentText, uint? currentSequenceNumber) =>
        activeGeneration == claim.Generation
        && claim.SequenceNumber is { } expected
        && currentSequenceNumber == expected
        && currentText == claim.ExpectedText;

    public void Complete(ClipboardClaim claim)
    {
        if (activeGeneration == claim.Generation)
        {
            activeGeneration = null;
        }
    }

    public bool IsActive(ClipboardClaim claim) => activeGeneration == claim.Generation;
}
