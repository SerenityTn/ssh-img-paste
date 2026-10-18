use ssh_img_paste_core::{ParseError, parse_profile};

const APP: &str = include_str!("../../../contract/profiles/app-literal.env");
const MANUAL_COMMAND: &str = include_str!("../../../contract/profiles/manual-command.env");
const DYNAMIC_SUPPORTED: &str = include_str!("../../../contract/profiles/dynamic-supported.env");
const MANUAL_DYNAMIC_EXTRA: &str =
    include_str!("../../../contract/profiles/manual-dynamic-extra.env");

#[test]
fn parses_app_owned_literal_fixture() {
    let profile = parse_profile(APP).expect("literal profile");
    assert!(profile.editable);
    assert_eq!(profile.label.as_deref(), Some("Development Host"));
    assert_eq!(profile.host.as_deref(), Some("dev-host"));
    assert_eq!(profile.remote_home.as_deref(), Some("/srv/dev"));
    assert_eq!(profile.remote_dir.as_deref(), Some("dev-images"));
    assert_eq!(profile.shot_mode.as_deref(), Some("full"));
    assert_eq!(profile.restore_seconds.as_deref(), Some("00007"));
}

#[test]
fn unsupported_command_is_never_executed_and_marks_manual() {
    let profile = parse_profile(MANUAL_COMMAND).expect("inspectable manual profile");
    assert!(!profile.editable);
    assert_eq!(profile.host.as_deref(), Some("manual-host"));
}

#[test]
fn dynamic_supported_assignment_is_a_clear_error() {
    assert_eq!(
        parse_profile(DYNAMIC_SUPPORTED),
        Err(ParseError::DynamicSupportedAssignment("SSH_HOST".into()))
    );
}

#[test]
fn dynamic_unknown_assignment_is_manual_but_supported_literals_survive() {
    let profile = parse_profile(MANUAL_DYNAMIC_EXTRA).expect("manual profile");
    assert!(!profile.editable);
    assert_eq!(profile.label.as_deref(), Some("Manual Extra"));
    assert_eq!(profile.host.as_deref(), Some("literal-host"));
}
