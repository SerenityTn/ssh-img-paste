use ssh_img_paste_core::ClipboardCoordinator;

#[test]
fn restores_only_when_the_transaction_still_owns_the_clipboard_value() {
    let mut coordinator = ClipboardCoordinator::new();
    let transaction = coordinator.begin("/srv/images/clip-a.png", Some(42));

    assert!(coordinator.should_restore(&transaction, Some("/srv/images/clip-a.png"), Some(42)));
    assert!(!coordinator.should_restore(
        &transaction,
        Some("user copied something else"),
        Some(42)
    ));
    assert!(!coordinator.should_restore(&transaction, None, Some(42)));
    assert!(!coordinator.should_restore(&transaction, Some("/srv/images/clip-a.png"), Some(43)));
}

#[test]
fn a_new_upload_supersedes_an_older_restoration_timer() {
    let mut coordinator = ClipboardCoordinator::new();
    let older = coordinator.begin("/srv/images/clip-a.png", Some(10));
    let newer = coordinator.begin("/srv/images/clip-b.png", Some(20));

    assert!(!coordinator.should_restore(&older, Some("/srv/images/clip-a.png"), Some(10)));
    coordinator.complete(&older);
    assert!(coordinator.should_restore(&newer, Some("/srv/images/clip-b.png"), Some(20)));
}

#[test]
fn stale_cancellation_cannot_clear_a_newer_transaction() {
    let mut coordinator = ClipboardCoordinator::new();
    let older = coordinator.begin("/srv/images/clip-a.png", Some(10));
    let newer = coordinator.begin("/srv/images/clip-b.png", Some(20));

    coordinator.cancel(&older);

    assert!(coordinator.should_restore(&newer, Some("/srv/images/clip-b.png"), Some(20)));
}

#[test]
fn restoration_fails_closed_without_a_platform_ownership_marker() {
    let mut coordinator = ClipboardCoordinator::new();
    let untracked = coordinator.begin("/srv/images/clip-a.png", None);

    assert!(!coordinator.should_restore(&untracked, Some("/srv/images/clip-a.png"), None));
}

#[test]
fn completing_or_cancelling_the_active_transaction_invalidates_restoration() {
    let mut coordinator = ClipboardCoordinator::new();
    let completed = coordinator.begin("/srv/images/clip-a.png", Some(10));
    coordinator.complete(&completed);
    assert!(!coordinator.should_restore(&completed, Some("/srv/images/clip-a.png"), Some(10)));

    let cancelled = coordinator.begin("/srv/images/clip-b.png", Some(20));
    coordinator.cancel(&cancelled);
    assert!(!coordinator.should_restore(&cancelled, Some("/srv/images/clip-b.png"), Some(20)));
}
