//! Read-only probes and remote upload-directory operations.
//!
//! Every plan here interpolates only values that already passed the strict
//! allowlists in the parent module into constant remote commands, matching
//! the upload planner's shell-boundary rules.

use super::{
    CommandPlan, PlanError, ValidatedProfile, revalidate_for_plan, ssh_plan, valid_absolute_path,
    valid_host, valid_remote_name,
};

/// Remote command that reports the login account's home directory.
const PROBE_COMMAND: &str = "printf '%s\\n' \"$HOME\"";

/// Maximum number of listing rows accepted from a remote host.
pub const MAX_LISTED_UPLOADS: usize = 5000;

fn remote_root(profile: &ValidatedProfile) -> String {
    let remote_home = profile.remote_home.trim_end_matches('/');
    let remote_dir = profile.remote_dir.trim_end_matches('/');
    if remote_home.is_empty() {
        format!("/{remote_dir}")
    } else {
        format!("{remote_home}/{remote_dir}")
    }
}

/// The absolute remote upload directory a validated profile targets.
pub fn remote_upload_root(profile: &ValidatedProfile) -> Result<String, PlanError> {
    revalidate_for_plan(profile)?;
    Ok(remote_root(profile))
}

fn valid_plan_host(host: &str) -> bool {
    valid_host(host) && !host.contains(':')
}

/// Plans a batch-mode connection probe that prints the remote `$HOME`.
///
/// It accepts a bare host so a destination can be checked before it is saved.
pub fn build_probe_plan(host: &str) -> Result<CommandPlan, PlanError> {
    if !valid_plan_host(host) {
        return Err(PlanError::InvalidProfileField("host"));
    }
    Ok(ssh_plan(host, PROBE_COMMAND.to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutputError {
    /// The host answered, but not with a home directory this app can use.
    UnusableHome,
}

/// Extracts the remote home from probe output.
///
/// Login banners or shell start-up noise may precede the value, so the last
/// non-empty line is used, and it must satisfy the same rules as a saved
/// `SSH_REMOTE_HOME`.
pub fn parse_probe_output(stdout: &[u8]) -> Result<String, ProbeOutputError> {
    let text = std::str::from_utf8(stdout).map_err(|_| ProbeOutputError::UnusableHome)?;
    let candidate = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .last()
        .ok_or(ProbeOutputError::UnusableHome)?;
    let candidate = if candidate.len() > 1 {
        candidate.trim_end_matches('/')
    } else {
        candidate
    };
    if valid_absolute_path(candidate) {
        Ok(candidate.to_owned())
    } else {
        Err(ProbeOutputError::UnusableHome)
    }
}

/// Plans a listing of PNG uploads in the profile's remote directory.
///
/// A missing directory lists as empty instead of being created, so listing
/// stays read-only. Output rows are `MTIME<TAB>SIZE<TAB>NAME` and require GNU
/// `find`, matching the macOS edition.
pub fn build_list_plan(profile: &ValidatedProfile) -> Result<CommandPlan, PlanError> {
    let root = remote_upload_root(profile)?;
    Ok(ssh_plan(
        &profile.host,
        format!(
            "test -d {root} || exit 0; find {root} -maxdepth 1 -type f -name '*.png' -printf '%T@\\t%s\\t%f\\n'"
        ),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUpload {
    pub name: String,
    pub size_bytes: u64,
    pub modified_epoch_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteListing {
    /// Valid uploads, newest first, ties broken by name.
    pub uploads: Vec<RemoteUpload>,
    /// Rows that did not match the expected shape or name rules.
    pub skipped_rows: usize,
    /// True when more than `MAX_LISTED_UPLOADS` valid rows were returned.
    pub truncated: bool,
}

fn parse_epoch_seconds(value: &str) -> Option<i64> {
    let whole = value.split_once('.').map_or(value, |(whole, _)| whole);
    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some((_, fraction)) = value.split_once('.') {
        if !fraction.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    whole.parse().ok()
}

/// Parses listing output into validated upload rows.
pub fn parse_listing(stdout: &[u8]) -> RemoteListing {
    let text = String::from_utf8_lossy(stdout);
    let mut listing = RemoteListing::default();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(modified), Some(size), Some(name)) = (fields.next(), fields.next(), fields.next())
        else {
            listing.skipped_rows += 1;
            continue;
        };
        let Some(modified_epoch_seconds) = parse_epoch_seconds(modified) else {
            listing.skipped_rows += 1;
            continue;
        };
        let Ok(size_bytes) = size.parse::<u64>() else {
            listing.skipped_rows += 1;
            continue;
        };
        if !valid_remote_name(name) {
            listing.skipped_rows += 1;
            continue;
        }
        listing.uploads.push(RemoteUpload {
            name: name.to_owned(),
            size_bytes,
            modified_epoch_seconds,
        });
    }
    listing.uploads.sort_by(|left, right| {
        right
            .modified_epoch_seconds
            .cmp(&left.modified_epoch_seconds)
            .then_with(|| left.name.cmp(&right.name))
    });
    if listing.uploads.len() > MAX_LISTED_UPLOADS {
        listing.uploads.truncate(MAX_LISTED_UPLOADS);
        listing.truncated = true;
    }
    listing
}

/// Returns whether a basename may be fetched from the upload directory.
pub fn valid_upload_name(name: &str) -> bool {
    valid_remote_name(name)
}

/// Plans an SCP download of one upload into an absolute local file path.
pub fn build_fetch_plan(
    profile: &ValidatedProfile,
    name: &str,
    destination: &std::path::Path,
) -> Result<CommandPlan, PlanError> {
    let root = remote_upload_root(profile)?;
    if !valid_remote_name(name) {
        return Err(PlanError::InvalidRemoteName);
    }
    if !destination.is_absolute() || destination.as_os_str().to_string_lossy().starts_with('-') {
        return Err(PlanError::InvalidSource);
    }
    Ok(CommandPlan {
        program: "scp".to_owned(),
        arguments: vec![
            "-q".into(),
            "-B".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ConnectTimeout=6".into(),
            "--".into(),
            format!("{}:{root}/{name}", profile.host).into(),
            destination.as_os_str().to_owned(),
        ],
    })
}

/// Plans deletion of every PNG upload directly inside the remote directory.
///
/// Prints one byte per deleted file followed by a newline-terminated count,
/// so the caller can report how many were removed.
pub fn build_clean_plan(profile: &ValidatedProfile) -> Result<CommandPlan, PlanError> {
    let root = remote_upload_root(profile)?;
    Ok(ssh_plan(
        &profile.host,
        format!(
            "test -d {root} || {{ echo 0; exit 0; }}; find {root} -maxdepth 1 -type f -name '*.png' -printf . -delete | wc -c"
        ),
    ))
}

/// Parses the deleted-file count printed by the clean plan.
pub fn parse_clean_output(stdout: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(stdout).ok()?;
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()?
        .parse()
        .ok()
}
