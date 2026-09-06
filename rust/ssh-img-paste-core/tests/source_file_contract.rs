use ssh_img_paste_core::{SourceFileError, generate_remote_name, open_upload_source};
use std::io::Read;
use std::path::PathBuf;

fn temporary_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-source-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary root");
    root
}

#[test]
fn generated_remote_name_is_deterministic_and_plan_safe() {
    assert_eq!(
        generate_remote_name(
            1_786_032_000,
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        ),
        "ssh-img-1786032000-0123456789abcdef.png"
    );
}

#[test]
fn regular_source_is_opened_and_retained_for_staging() {
    let root = temporary_root();
    let source = root.join("image.png");
    std::fs::write(&source, b"\x89PNG\r\n\x1a\nsource bytes").expect("source file");

    let mut opened = open_upload_source(&source).expect("regular source");
    std::fs::remove_file(&source).expect("remove pathname after opening");
    let mut bytes = Vec::new();
    opened
        .read_to_end(&mut bytes)
        .expect("read retained source");

    assert_eq!(bytes, b"\x89PNG\r\n\x1a\nsource bytes");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn non_png_regular_source_is_rejected() {
    let root = temporary_root();
    let source = root.join("not-an-image.png");
    std::fs::write(&source, b"not a PNG").expect("non-PNG source");

    assert!(matches!(
        open_upload_source(&source),
        Err(SourceFileError::InvalidPng)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn directory_source_is_rejected() {
    let root = temporary_root();

    assert!(matches!(
        open_upload_source(&root),
        Err(SourceFileError::NotRegularFile)
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_source_is_not_followed() {
    use std::os::unix::fs::symlink;

    let root = temporary_root();
    let outside = root.join("outside.png");
    let linked = root.join("linked.png");
    std::fs::write(&outside, b"\x89PNG\r\n\x1a\noutside bytes").expect("outside source");
    symlink(&outside, &linked).expect("source symlink");

    assert!(open_upload_source(&linked).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn source_with_a_symlinked_parent_is_not_followed() {
    use std::os::unix::fs::symlink;

    let root = temporary_root();
    let real_parent = root.join("real-parent");
    std::fs::create_dir(&real_parent).expect("real parent");
    std::fs::write(real_parent.join("image.png"), b"\x89PNG\r\n\x1a\nbytes").expect("source image");
    let linked_parent = root.join("linked-parent");
    symlink(&real_parent, &linked_parent).expect("parent symlink");

    assert!(open_upload_source(&linked_parent.join("image.png")).is_err());
    let _ = std::fs::remove_dir_all(root);
}
