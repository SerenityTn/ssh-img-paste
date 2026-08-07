use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ssh_img_paste_core::{PlanError, ValidatedProfile, build_upload_plan};

fn profile() -> ValidatedProfile {
    ValidatedProfile {
        label: "Work".into(),
        host: "work-host".into(),
        remote_home: "/srv/me".into(),
        remote_dir: "images".into(),
        shot_mode: Some("region".into()),
        restore_seconds: Some("60".into()),
        editable: true,
    }
}

fn strings(values: &[impl AsRef<OsStr>]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.as_ref().to_string_lossy().into_owned())
        .collect()
}

fn local_source(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\Temp\{name}"))
    } else {
        PathBuf::from(format!("/tmp/{name}"))
    }
}

#[test]
fn plans_mkdir_partial_upload_and_atomic_finalize_as_argument_arrays() {
    let source = local_source("image with spaces.png");
    let plan = build_upload_plan(&profile(), &source, "clip-20260807-170000-a31f62c8.png")
        .expect("safe upload plan");

    assert_eq!(
        plan.remote_path,
        "/srv/me/images/clip-20260807-170000-a31f62c8.png"
    );
    assert_eq!(plan.mkdir.program, "ssh");
    assert_eq!(
        strings(&plan.mkdir.arguments),
        [
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=6",
            "work-host",
            "mkdir -p -- /srv/me/images",
        ]
    );

    assert_eq!(plan.upload.program, "scp");
    let upload = strings(&plan.upload.arguments);
    assert_eq!(upload[6], "--");
    assert_eq!(upload[7], source.to_string_lossy());
    assert_eq!(
        upload[8],
        "work-host:/srv/me/images/.clip-20260807-170000-a31f62c8.png.partial"
    );
    assert_eq!(plan.upload.arguments[7], source.as_os_str());

    assert_eq!(plan.finalize.program, "ssh");
    assert_eq!(
        strings(&plan.finalize.arguments)[5],
        "mv -- /srv/me/images/.clip-20260807-170000-a31f62c8.png.partial /srv/me/images/clip-20260807-170000-a31f62c8.png"
    );
}

#[test]
fn rejects_remote_names_that_cross_the_remote_shell_boundary() {
    for name in [
        "../escape.png",
        "-option.png",
        "image name.png",
        "image;touch-pwn.png",
        "image.jpg",
        "image..png",
    ] {
        assert_eq!(
            build_upload_plan(&profile(), &local_source("image.png"), name),
            Err(PlanError::InvalidRemoteName),
            "accepted {name:?}"
        );
    }
}

#[test]
fn rejects_option_like_source_paths_before_scp_planning() {
    for source in ["-Fattacker-config", "attacker.example:/secret.png"] {
        assert_eq!(
            build_upload_plan(&profile(), Path::new(source), "safe.png"),
            Err(PlanError::InvalidSource),
            "accepted source {source:?}"
        );
    }
}

#[test]
fn normalizes_accepted_trailing_remote_separators() {
    let mut trailing = profile();
    trailing.remote_home = "/srv/me/".into();
    trailing.remote_dir = "images/".into();

    let plan = build_upload_plan(&trailing, &local_source("image.png"), "safe.png")
        .expect("normalized upload plan");

    assert_eq!(plan.remote_path, "/srv/me/images/safe.png");
    assert!(!plan.remote_path.contains("//"));
}

#[test]
fn revalidates_profile_fields_before_creating_any_command() {
    for unsafe_host in ["-oProxyCommand=evil", "work-host:2222", "host/segment"] {
        let mut unsafe_profile = profile();
        unsafe_profile.host = unsafe_host.into();
        assert_eq!(
            build_upload_plan(&unsafe_profile, &local_source("image.png"), "safe.png"),
            Err(PlanError::InvalidProfileField("host")),
            "accepted host {unsafe_host:?}"
        );
    }

    let mut dot_home = profile();
    dot_home.remote_home = "/srv/./me".into();
    assert_eq!(
        build_upload_plan(&dot_home, &local_source("image.png"), "safe.png"),
        Err(PlanError::InvalidProfileField("remote_home"))
    );

    let mut dot_dir = profile();
    dot_dir.remote_dir = "images/.".into();
    assert_eq!(
        build_upload_plan(&dot_dir, &local_source("image.png"), "safe.png"),
        Err(PlanError::InvalidProfileField("remote_dir"))
    );
}
