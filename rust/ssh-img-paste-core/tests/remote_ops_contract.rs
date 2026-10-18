use ssh_img_paste_core::{
    PlanError, ProfileId, ValidatedProfile, build_clean_plan, build_fetch_plan, build_list_plan,
    build_probe_plan, parse_clean_output, parse_listing, parse_probe_output, parse_profile,
    validate_profile,
};
use std::ffi::OsString;

fn profile() -> ValidatedProfile {
    let document = parse_profile(
        "SSH_HOST='work-ssh'\nSSH_REMOTE_HOME='/home/me/'\nSSH_REMOTE_DIR='img-uploads'\n",
    )
    .expect("parse");
    validate_profile(&ProfileId::parse("work").expect("id"), document).expect("validate")
}

fn strings(arguments: &[OsString]) -> Vec<String> {
    arguments
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn probe_uses_batch_mode_and_a_constant_remote_command() {
    let plan = build_probe_plan("me@work-ssh").expect("plan");
    assert_eq!(plan.program(), "ssh");
    let arguments = strings(plan.arguments());
    assert_eq!(
        arguments[..5],
        ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "me@work-ssh"]
    );
    assert_eq!(arguments[5], "printf '%s\\n' \"$HOME\"");
    for hostile in ["-oProxyCommand=calc", "host;rm", "host:22", "", "a b"] {
        assert!(
            matches!(build_probe_plan(hostile), Err(PlanError::InvalidProfileField("host"))),
            "{hostile:?} must be rejected"
        );
    }
}

#[test]
fn probe_output_uses_the_last_line_and_requires_a_safe_absolute_home() {
    assert_eq!(parse_probe_output(b"/home/me\n").as_deref(), Ok("/home/me"));
    assert_eq!(
        parse_probe_output(b"Welcome banner\r\n/srv/users/me/\r\n").as_deref(),
        Ok("/srv/users/me")
    );
    assert_eq!(parse_probe_output(b"/\n").as_deref(), Ok("/"));
    for bad in [&b""[..], b"\n", b"relative\n", b"/home/with space\n", b"/home/$(x)\n", b"C:/Users/me\n"] {
        assert!(parse_probe_output(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn listing_is_read_only_and_scoped_to_the_upload_directory() {
    let plan = build_list_plan(&profile()).expect("plan");
    let arguments = strings(plan.arguments());
    assert_eq!(arguments[4], "work-ssh");
    assert_eq!(
        arguments[5],
        "test -d /home/me/img-uploads || exit 0; find /home/me/img-uploads -maxdepth 1 -type f -name '*.png' -printf '%T@\\t%s\\t%f\\n'"
    );
    assert!(!arguments[5].contains("mkdir"));
}

#[test]
fn listing_rows_are_validated_sorted_newest_first_and_bad_rows_skipped() {
    let output = b"1700000000.5\t2048\tolder.png\n\
1700000900.0000000000\t10\tnewest.png\n\
1700000900\t11\tanother.png\n\
not-a-time\t1\tbad-time.png\n\
1700000000\tbig\tbad-size.png\n\
1700000000\t1\tspaced name.png\n\
1700000000\t1\t-dash.png\n\
1700000000\t1\n";
    let listing = parse_listing(output);
    let names: Vec<_> = listing.uploads.iter().map(|upload| upload.name.as_str()).collect();
    assert_eq!(names, ["another.png", "newest.png", "older.png"]);
    assert_eq!(listing.uploads[2].size_bytes, 2048);
    assert_eq!(listing.uploads[2].modified_epoch_seconds, 1_700_000_000);
    assert_eq!(listing.skipped_rows, 5);
    assert!(!listing.truncated);
    assert!(parse_listing(b"").uploads.is_empty());
}

#[test]
fn fetch_validates_the_basename_and_local_destination() {
    let destination = std::env::temp_dir().join("fetched.png");
    let plan = build_fetch_plan(&profile(), "shot-1.png", &destination).expect("plan");
    assert_eq!(plan.program(), "scp");
    let arguments = strings(plan.arguments());
    assert_eq!(arguments[6], "--");
    assert_eq!(arguments[7], "work-ssh:/home/me/img-uploads/shot-1.png");
    assert_eq!(arguments[8], destination.to_string_lossy());
    for name in ["../etc/passwd.png", "a/b.png", "x.txt", "-o.png", "", "a b.png"] {
        assert!(
            matches!(
                build_fetch_plan(&profile(), name, &destination),
                Err(PlanError::InvalidRemoteName)
            ),
            "{name:?}"
        );
    }
    assert!(matches!(
        build_fetch_plan(&profile(), "ok.png", std::path::Path::new("relative.png")),
        Err(PlanError::InvalidSource)
    ));
}

#[test]
fn clean_deletes_only_top_level_pngs_and_reports_a_count() {
    let plan = build_clean_plan(&profile()).expect("plan");
    let arguments = strings(plan.arguments());
    assert_eq!(
        arguments[5],
        "test -d /home/me/img-uploads || { echo 0; exit 0; }; find /home/me/img-uploads -maxdepth 1 -type f -name '*.png' -printf . -delete | wc -c"
    );
    assert_eq!(parse_clean_output(b"      3\n"), Some(3));
    assert_eq!(parse_clean_output(b"0\n"), Some(0));
    assert_eq!(parse_clean_output(b"junk\n"), None);
    assert_eq!(parse_clean_output(b""), None);
}
