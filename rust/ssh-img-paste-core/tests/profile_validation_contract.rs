use ssh_img_paste_core::{ProfileId, ValidationError, parse_profile, validate_profile};

const APP: &str = include_str!("../../../contract/profiles/app-literal.env");

#[test]
fn validates_literal_fixture_and_preserves_zero_padding() {
    let id = ProfileId::parse("dev").unwrap();
    let profile = validate_profile(&id, parse_profile(APP).unwrap()).expect("valid profile");
    assert_eq!(profile.label, "Development Host");
    assert_eq!(profile.host, "dev-host");
    assert_eq!(profile.remote_home, "/srv/dev");
    assert_eq!(profile.remote_dir, "dev-images");
    assert_eq!(profile.restore_seconds.as_deref(), Some("00007"));
}

#[test]
fn rejects_adversarial_fields_before_adapters_run() {
    let id = ProfileId::parse("dev").unwrap();
    let base = parse_profile(APP).unwrap();
    let cases = [
        ("host", "-option"),
        ("host", "host name"),
        ("host", "$(touch-pwn)"),
        ("remote_home", "srv/dev"),
        ("remote_home", "/srv/../dev"),
        ("remote_dir", "../images"),
        ("remote_dir", "images;touch-pwn"),
        ("shot_mode", "window"),
        ("restore_seconds", "86401"),
        ("restore_seconds", "999999999999999999999999999999999"),
    ];

    for (field, value) in cases {
        let mut candidate = base.clone();
        match field {
            "host" => candidate.host = Some(value.into()),
            "remote_home" => candidate.remote_home = Some(value.into()),
            "remote_dir" => candidate.remote_dir = Some(value.into()),
            "shot_mode" => candidate.shot_mode = Some(value.into()),
            "restore_seconds" => candidate.restore_seconds = Some(value.into()),
            _ => unreachable!(),
        }
        assert_eq!(
            validate_profile(&id, candidate),
            Err(ValidationError::InvalidField(field)),
            "accepted {field}={value:?}"
        );
    }
}

#[test]
fn empty_label_falls_back_to_profile_id() {
    let id = ProfileId::parse("fallback").unwrap();
    let mut document = parse_profile(APP).unwrap();
    document.label = Some(String::new());
    assert_eq!(validate_profile(&id, document).unwrap().label, "fallback");
}
