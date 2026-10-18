use ssh_img_paste_core::discover_profiles;
use std::path::PathBuf;

fn temporary_catalog() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-profile-discovery-{}-{:?}",
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

#[cfg(unix)]
#[test]
fn discovery_rejects_a_symlinked_profiles_directory() {
    use std::os::unix::fs::symlink;

    let root = temporary_catalog();
    let real_profiles = root.join("real-profiles");
    std::fs::rename(root.join("profiles"), &real_profiles).expect("move profiles directory");
    std::fs::write(
        real_profiles.join("linked.env"),
        "SSH_HOST='outside@example.test'\nSSH_REMOTE_HOME='/home/user'\nSSH_REMOTE_DIR='img-uploads'\n",
    )
    .expect("write linked directory profile");
    symlink(&real_profiles, root.join("profiles")).expect("create profiles directory symlink");

    assert!(
        discover_profiles(&root).is_err(),
        "profile discovery must not follow the profiles directory"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn discovery_rejects_a_symlinked_profile() {
    use std::os::unix::fs::symlink;

    let root = temporary_catalog();
    let outside = root.join("outside.env");
    std::fs::write(
        &outside,
        "SSH_HOST='outside@example.test'\nSSH_REMOTE_HOME='/home/user'\nSSH_REMOTE_DIR='img-uploads'\n",
    )
    .expect("write outside profile");
    symlink(&outside, root.join("profiles").join("linked.env")).expect("create profile symlink");

    assert!(
        discover_profiles(&root).is_err(),
        "profile discovery must not follow a symlink"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn discovery_returns_valid_profiles_in_ascii_id_order() {
    let root = temporary_catalog();
    write_profile(&root, "zeta", "zeta@example.test");
    write_profile(&root, "Alpha", "alpha@example.test");
    write_profile(&root, "middle-2", "middle@example.test");

    let catalog = discover_profiles(&root).expect("discover profiles");
    let ids: Vec<_> = catalog
        .profiles
        .iter()
        .map(|profile| profile.id.as_str())
        .collect();

    assert_eq!(ids, ["Alpha", "middle-2", "zeta"]);
    assert_eq!(catalog.profiles[0].profile.host, "alpha@example.test");
    let _ = std::fs::remove_dir_all(root);
}
