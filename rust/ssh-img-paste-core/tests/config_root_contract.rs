use ssh_img_paste_core::{ConfigRootError, default_config_root};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn env(values: &[(&str, &str)]) -> BTreeMap<String, OsString> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
        .collect()
}

fn absolute_override() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\ssh-img-paste-test")
    } else {
        PathBuf::from("/tmp/ssh-img-paste-test")
    }
}

#[test]
fn absolute_override_wins_without_environment_lookup() {
    let expected = absolute_override();
    let root = default_config_root(Some(&expected), &BTreeMap::new()).expect("absolute override");

    assert_eq!(root, expected);
}

#[test]
fn relative_override_is_rejected() {
    assert_eq!(
        default_config_root(Some(Path::new("relative/config")), &BTreeMap::new()),
        Err(ConfigRootError::PathMustBeAbsolute("--config-root")),
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_uses_absolute_xdg_config_home() {
    let root = default_config_root(
        None,
        &env(&[
            ("XDG_CONFIG_HOME", "/var/lib/example-config"),
            ("HOME", "/home/ignored"),
        ]),
    )
    .expect("Linux config root");

    assert_eq!(root, PathBuf::from("/var/lib/example-config/ssh-img-paste"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_falls_back_to_home_dot_config() {
    let root =
        default_config_root(None, &env(&[("HOME", "/home/ada")])).expect("Linux home fallback");

    assert_eq!(root, PathBuf::from("/home/ada/.config/ssh-img-paste"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_rejects_relative_xdg_config_home() {
    assert_eq!(
        default_config_root(
            None,
            &env(&[("XDG_CONFIG_HOME", "relative"), ("HOME", "/home/ada")]),
        ),
        Err(ConfigRootError::PathMustBeAbsolute("XDG_CONFIG_HOME")),
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_uses_absolute_local_app_data() {
    let root = default_config_root(
        None,
        &env(&[
            ("LOCALAPPDATA", r"C:\Users\Ada\AppData\Local"),
            ("USERPROFILE", r"C:\Users\Ignored"),
        ]),
    )
    .expect("Windows config root");

    assert_eq!(
        root,
        PathBuf::from(r"C:\Users\Ada\AppData\Local\SSH Image Paste")
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_falls_back_to_user_profile() {
    let root = default_config_root(None, &env(&[("USERPROFILE", r"C:\Users\Ada")]))
        .expect("Windows profile fallback");

    assert_eq!(
        root,
        PathBuf::from(r"C:\Users\Ada\AppData\Local\SSH Image Paste")
    );
}

#[test]
fn missing_platform_home_is_a_clear_error() {
    assert_eq!(
        default_config_root(None, &BTreeMap::new()),
        Err(ConfigRootError::MissingEnvironment(
            if cfg!(target_os = "windows") {
                "LOCALAPPDATA or USERPROFILE"
            } else if cfg!(target_os = "linux") {
                "XDG_CONFIG_HOME or HOME"
            } else {
                "supported platform"
            }
        )),
    );
}
