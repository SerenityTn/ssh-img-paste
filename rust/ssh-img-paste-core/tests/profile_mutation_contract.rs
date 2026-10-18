use ssh_img_paste_core::{
    CatalogEntryStatus, MutationError, ProfileFields, ProfileId, ProfileSelection,
    ProfileStoreError, UnusableReason, create_profile, delete_profile, inspect_catalog,
    rename_profile, resolve_profile, set_active_profile, update_profile,
};
use std::path::{Path, PathBuf};

fn temporary_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-mutation-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn id(value: &str) -> ProfileId {
    ProfileId::parse(value).expect("valid id")
}

fn fields(label: &str, host: &str) -> ProfileFields {
    ProfileFields {
        label: Some(label.into()),
        host: Some(host.into()),
        remote_home: Some("/home/me".into()),
        remote_dir: Some("img-uploads".into()),
        ..ProfileFields::default()
    }
}

fn active(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join("active-profile"))
        .ok()
        .map(|value| value.trim_end().to_owned())
}

#[test]
fn first_created_profile_becomes_active_and_later_ones_do_not() {
    let root = temporary_root("create");
    let first = create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    assert!(first.activated);
    let second = create_profile(&root, &id("home"), &fields("Home", "home-ssh")).expect("create");
    assert!(!second.activated);
    assert_eq!(active(&root).as_deref(), Some("work"));

    let resolved = resolve_profile(&root, ProfileSelection::Active).expect("resolve");
    assert_eq!(resolved.id.as_str(), "work");
    assert_eq!(resolved.profile.host, "work-ssh");
    assert!(resolved.profile.editable);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_rejects_duplicates_missing_fields_and_invalid_values_before_writing() {
    let root = temporary_root("create-reject");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    assert!(matches!(
        create_profile(&root, &id("work"), &fields("Again", "x")),
        Err(MutationError::ProfileExists(_))
    ));

    let mut missing = fields("Missing", "host");
    missing.remote_dir = None;
    assert!(matches!(
        create_profile(&root, &id("missing"), &missing),
        Err(MutationError::MissingField("remote_dir"))
    ));

    let mut windows_home = fields("Windows", "host");
    windows_home.remote_home = Some("C:/Users/me".into());
    assert!(matches!(
        create_profile(&root, &id("windows"), &windows_home),
        Err(MutationError::InvalidField("remote_home"))
    ));

    let hostile = fields("Hostile", "-oProxyCommand=calc");
    assert!(matches!(
        create_profile(&root, &id("hostile"), &hostile),
        Err(MutationError::InvalidField("host"))
    ));

    let names: Vec<_> = std::fs::read_dir(root.join("profiles"))
        .expect("profiles")
        .map(|entry| entry.expect("entry").file_name().into_string().expect("utf8"))
        .collect();
    assert_eq!(names, ["work.env"], "rejected creates must leave no files");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn update_rewrites_only_requested_fields_and_refuses_manual_profiles() {
    let root = temporary_root("update");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    update_profile(
        &root,
        &id("work"),
        &ProfileFields {
            label: Some("Work Server".into()),
            shot_mode: Some("full".into()),
            restore_seconds: Some("30".into()),
            ..ProfileFields::default()
        },
    )
    .expect("update");
    let resolved = resolve_profile(&root, ProfileSelection::Explicit(id("work"))).expect("resolve");
    assert_eq!(resolved.profile.label, "Work Server");
    assert_eq!(resolved.profile.host, "work-ssh");
    assert_eq!(resolved.profile.shot_mode.as_deref(), Some("full"));
    assert_eq!(resolved.profile.restore_seconds.as_deref(), Some("30"));

    assert!(matches!(
        update_profile(&root, &id("work"), &ProfileFields::default()),
        Err(MutationError::NoFieldsToUpdate)
    ));
    assert!(matches!(
        update_profile(
            &root,
            &id("work"),
            &ProfileFields {
                remote_dir: Some("../escape".into()),
                ..ProfileFields::default()
            }
        ),
        Err(MutationError::InvalidField("remote_dir"))
    ));

    std::fs::write(
        root.join("profiles").join("manual.env"),
        "export SSH_HOST=manual\nSSH_REMOTE_HOME=/home/me\nSSH_REMOTE_DIR=img\n",
    )
    .expect("manual profile");
    assert!(matches!(
        update_profile(
            &root,
            &id("manual"),
            &ProfileFields {
                label: Some("Changed".into()),
                ..ProfileFields::default()
            }
        ),
        Err(MutationError::ProfileNotEditable(_))
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_carries_the_active_selection() {
    let root = temporary_root("rename");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    create_profile(&root, &id("home"), &fields("Home", "home-ssh")).expect("create");

    let outcome = rename_profile(&root, &id("work"), &id("client-a")).expect("rename");
    assert!(outcome.active_updated);
    assert_eq!(active(&root).as_deref(), Some("client-a"));
    assert!(!root.join("profiles").join("work.env").exists());

    let outcome = rename_profile(&root, &id("home"), &id("house")).expect("rename");
    assert!(!outcome.active_updated);
    assert!(matches!(
        rename_profile(&root, &id("house"), &id("client-a")),
        Err(MutationError::ProfileExists(_))
    ));
    assert!(matches!(
        rename_profile(&root, &id("absent"), &id("other")),
        Err(MutationError::ProfileNotFound(_))
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn delete_protects_the_active_and_last_usable_profiles() {
    let root = temporary_root("delete");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    assert!(matches!(
        delete_profile(&root, &id("work"), None),
        Err(MutationError::LastUsableProfile)
    ));
    create_profile(&root, &id("home"), &fields("Home", "home-ssh")).expect("create");
    assert!(matches!(
        delete_profile(&root, &id("work"), None),
        Err(MutationError::SwitchToRequired)
    ));
    assert!(matches!(
        delete_profile(&root, &id("work"), Some(&id("work"))),
        Err(MutationError::SwitchToSameProfile)
    ));
    let outcome = delete_profile(&root, &id("work"), Some(&id("home"))).expect("delete");
    assert_eq!(outcome.active_profile_id.as_ref().map(ProfileId::as_str), Some("home"));
    assert_eq!(active(&root).as_deref(), Some("home"));

    create_profile(&root, &id("spare"), &fields("Spare", "spare-ssh")).expect("create");
    delete_profile(&root, &id("spare"), None).expect("inactive delete needs no replacement");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn broken_profiles_are_reported_not_fatal_and_can_be_removed() {
    let root = temporary_root("broken");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    std::fs::write(
        root.join("profiles").join("dynamic.env"),
        "SSH_HOST=$(whoami)\nSSH_REMOTE_HOME=/home/me\nSSH_REMOTE_DIR=img\n",
    )
    .expect("dynamic profile");
    std::fs::write(
        root.join("profiles").join("invalid.env"),
        "SSH_HOST='ok'\nSSH_REMOTE_HOME='relative'\nSSH_REMOTE_DIR='img'\n",
    )
    .expect("invalid profile");

    let inspection = inspect_catalog(&root).expect("inspect");
    assert!(inspection.configured);
    assert_eq!(inspection.usable_count(), 1);
    let status = |name: &str| inspection.entry(&id(name)).expect("entry").status.clone();
    assert!(matches!(
        status("dynamic"),
        CatalogEntryStatus::Unusable(UnusableReason::DynamicSyntax(key)) if key == "SSH_HOST"
    ));
    assert!(matches!(
        status("invalid"),
        CatalogEntryStatus::Unusable(UnusableReason::InvalidField("remote_home"))
    ));
    assert!(matches!(
        set_active_profile(&root, &id("invalid")),
        Err(MutationError::ProfileUnusable(_))
    ));

    // An app-written profile with one bad field can be repaired in place.
    update_profile(
        &root,
        &id("invalid"),
        &ProfileFields {
            remote_home: Some("/home/me".into()),
            ..ProfileFields::default()
        },
    )
    .expect("repair");
    delete_profile(&root, &id("dynamic"), None).expect("broken profiles can be deleted");
    assert_eq!(inspect_catalog(&root).expect("inspect").usable_count(), 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deleting_a_broken_stored_active_profile_falls_back_cleanly() {
    let root = temporary_root("broken-active");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    std::fs::write(
        root.join("profiles").join("broken.env"),
        "SSH_HOST='ok'\nSSH_REMOTE_HOME='relative'\nSSH_REMOTE_DIR='img'\n",
    )
    .expect("broken profile");
    std::fs::write(root.join("active-profile"), "broken\n").expect("active");

    delete_profile(&root, &id("broken"), None).expect("delete");
    assert_eq!(active(&root), None);
    let resolved = resolve_profile(&root, ProfileSelection::Active).expect("resolve");
    assert_eq!(resolved.id.as_str(), "work");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mutations_on_a_missing_catalog_report_not_configured() {
    let root = temporary_root("missing");
    assert!(matches!(
        set_active_profile(&root, &id("work")),
        Err(MutationError::Store(ProfileStoreError::NotConfigured))
    ));
    let inspection = inspect_catalog(&root).expect("missing catalog inspects as empty");
    assert!(!inspection.configured);
    assert!(inspection.entries.is_empty());
}

#[test]
fn concurrent_creates_are_serialized_by_the_lock() {
    let root = temporary_root("concurrent");
    create_profile(&root, &id("seed"), &fields("Seed", "seed-ssh")).expect("seed");
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let root = root.clone();
            std::thread::spawn(move || {
                create_profile(
                    &root,
                    &id(&format!("p{index}")),
                    &fields(&format!("P{index}"), "host"),
                )
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("thread").expect("create");
    }
    let inspection = inspect_catalog(&root).expect("inspect");
    assert_eq!(inspection.usable_count(), 9);
    assert_eq!(active(&root).as_deref(), Some("seed"));
    let leftovers: Vec<_> = std::fs::read_dir(root.join("profiles"))
        .expect("profiles")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files must not remain");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn created_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = temporary_root("private");
    create_profile(&root, &id("work"), &fields("Work", "work-ssh")).expect("create");
    let mode = |path: PathBuf| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(root.join("profiles").join("work.env")), 0o600);
    assert_eq!(mode(root.join("active-profile")), 0o600);
    assert_eq!(mode(root.join("profiles")), 0o700);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hosts_with_a_port_separator_are_rejected_before_saving() {
    let root = temporary_root("colon");
    assert!(matches!(
        create_profile(&root, &id("ported"), &fields("Ported", "host:22")),
        Err(MutationError::InvalidField("host"))
    ));
    let _ = std::fs::remove_dir_all(root);
}
