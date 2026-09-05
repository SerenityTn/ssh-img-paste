use ssh_img_paste_core::{ProfileSelection, resolve_profile};
use std::path::PathBuf;

fn temporary_catalog() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-profile-selection-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("profiles")).expect("create profile catalog");
    root
}

fn write_profile(root: &std::path::Path, id: &str, host: &str) {
    std::fs::write(
        root.join("profiles").join(format!("{id}.env")),
        format!(
            "SSH_PROFILE_LABEL='{id}'\nSSH_HOST='{host}'\nSSH_REMOTE_HOME='/home/user'\nSSH_REMOTE_DIR='img-uploads'\n"
        ),
    )
    .expect("write profile");
}

#[test]
fn missing_profile_directory_outranks_stale_active_state() {
    let root = temporary_catalog();
    std::fs::write(root.join("active-profile"), "missing\n").expect("write stale active state");
    std::fs::remove_dir_all(root.join("profiles")).expect("remove profiles directory");

    assert!(matches!(
        resolve_profile(&root, ProfileSelection::Active),
        Err(ssh_img_paste_core::ProfileStoreError::NotConfigured)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_profile_directory_is_not_configured() {
    let root = temporary_catalog();
    std::fs::remove_dir_all(root.join("profiles")).expect("remove profiles directory");

    assert!(matches!(
        resolve_profile(&root, ProfileSelection::Active),
        Err(ssh_img_paste_core::ProfileStoreError::NotConfigured)
    ));
    assert!(!root.join("active-profile").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_profile_overrides_invalid_active_state_without_mutation() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "alpha@example.test");
    std::fs::write(root.join("active-profile"), "../wrong\n").expect("write invalid active state");
    let before = std::fs::read(root.join("active-profile")).expect("snapshot active state");

    let selected = resolve_profile(
        &root,
        ProfileSelection::Explicit(
            ssh_img_paste_core::ProfileId::parse("alpha").expect("profile id"),
        ),
    )
    .expect("resolve explicit profile");

    assert_eq!(selected.id.as_str(), "alpha");
    assert_eq!(
        std::fs::read(root.join("active-profile")).expect("read active state"),
        before
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn valid_active_file_selects_exact_profile() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "alpha@example.test");
    write_profile(&root, "zeta", "zeta@example.test");
    std::fs::write(root.join("active-profile"), "zeta\n").expect("write active profile");

    let selected =
        resolve_profile(&root, ProfileSelection::Active).expect("resolve active profile");

    assert_eq!(selected.id.as_str(), "zeta");
    assert_eq!(selected.profile.host, "zeta@example.test");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_active_file_selects_first_sorted_profile_without_writing() {
    let root = temporary_catalog();
    write_profile(&root, "zeta", "zeta@example.test");
    write_profile(&root, "alpha", "alpha@example.test");

    let selected = resolve_profile(&root, ProfileSelection::Active).expect("resolve profile");

    assert_eq!(selected.id.as_str(), "alpha");
    assert_eq!(selected.profile.host, "alpha@example.test");
    assert!(!root.join("active-profile").exists());
    let _ = std::fs::remove_dir_all(root);
}
