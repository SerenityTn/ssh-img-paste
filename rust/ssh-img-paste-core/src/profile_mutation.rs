//! Locked, atomic profile catalog mutations.
//!
//! Mirrors the macOS v2 CLI rules: creating the first usable profile makes it
//! active, only app-written (literal) profiles can be rewritten, deleting the
//! active profile needs an explicit replacement, and the last usable profile
//! cannot be deleted. Every write goes to a private temporary file that is
//! then renamed into place while an exclusive OS file lock is held.

use crate::profile_store::{
    CatalogEntryStatus, CatalogInspection, ProfileStoreError, UnusableReason,
    inspect_open_catalog, open_profiles_dir, read_profile_source,
};
use crate::{ParseError, ProfileId, ValidationError, parse_profile, validate_profile};
use std::io::Write;
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = ".profiles.lock";
const ACTIVE_FILE: &str = "active-profile";
const PROFILE_HEADER: &str = "# ssh-img-paste profile v2";

/// App-owned profile fields. `None` means "not provided".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileFields {
    pub label: Option<String>,
    pub host: Option<String>,
    pub remote_home: Option<String>,
    pub remote_dir: Option<String>,
    pub shot_mode: Option<String>,
    pub restore_seconds: Option<String>,
}

impl ProfileFields {
    fn is_empty(&self) -> bool {
        self.assignments().iter().all(|(_, value)| value.is_none())
    }

    fn assignments(&self) -> [(&'static str, Option<&str>); 6] {
        [
            ("SSH_PROFILE_LABEL", self.label.as_deref()),
            ("SSH_HOST", self.host.as_deref()),
            ("SSH_REMOTE_HOME", self.remote_home.as_deref()),
            ("SSH_REMOTE_DIR", self.remote_dir.as_deref()),
            ("SSH_SHOT_MODE", self.shot_mode.as_deref()),
            ("SSH_CLIP_RESTORE_SECONDS", self.restore_seconds.as_deref()),
        ]
    }
}

#[derive(Debug)]
pub enum MutationError {
    MissingField(&'static str),
    NoFieldsToUpdate,
    InvalidField(&'static str),
    ProfileExists(ProfileId),
    ProfileNotFound(ProfileId),
    ProfileNotEditable(ProfileId),
    ProfileUnusable(ProfileId),
    LastUsableProfile,
    SwitchToRequired,
    SwitchToSameProfile,
    UnsafePath(PathBuf),
    Store(ProfileStoreError),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    /// The active profile changed, but a later step failed.
    PartiallyApplied {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl From<ProfileStoreError> for MutationError {
    fn from(error: ProfileStoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOutcome {
    pub activated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOutcome {
    pub active_updated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub active_profile_id: Option<ProfileId>,
}

fn io_error(operation: &'static str, path: PathBuf) -> impl FnOnce(std::io::Error) -> MutationError {
    move |source| MutationError::Io {
        operation,
        path,
        source,
    }
}

/// Serializes a value the same way as the macOS CLI's `shell_quote`.
fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        if matches!(character, '\\' | '"' | '$' | '`') {
            output.push('\\');
        }
        output.push(character);
    }
    output.push('"');
    output
}

fn render_new_profile(fields: &ProfileFields) -> String {
    let mut output = String::from(PROFILE_HEADER);
    output.push('\n');
    for (key, value) in fields.assignments() {
        if let Some(value) = value {
            output.push_str(key);
            output.push('=');
            output.push_str(&quote(value));
            output.push('\n');
        }
    }
    output
}

fn line_key(line: &str) -> Option<&str> {
    line.split_once('=').map(|(key, _)| key)
}

/// Replaces supported assignments in place and appends any that are missing,
/// preserving every other line, like the macOS CLI's profile update.
fn render_updated_profile(original: &str, fields: &ProfileFields) -> String {
    let assignments = fields.assignments();
    let mut replaced = [false; 6];
    let mut output = String::with_capacity(original.len() + 128);
    for line in original.lines() {
        let position = line_key(line).and_then(|key| {
            assignments
                .iter()
                .position(|(name, value)| *name == key && value.is_some())
        });
        match position {
            Some(index) => {
                let (key, value) = assignments[index];
                output.push_str(key);
                output.push('=');
                output.push_str(&quote(value.expect("assignment has value")));
                replaced[index] = true;
            }
            None => output.push_str(line),
        }
        output.push('\n');
    }
    for (index, (key, value)) in assignments.iter().enumerate() {
        if let (Some(value), false) = (value, replaced[index]) {
            output.push_str(key);
            output.push('=');
            output.push_str(&quote(value));
            output.push('\n');
        }
    }
    output
}

/// Confirms rendered content parses as literal, editable, and valid.
fn check_rendered(id: &ProfileId, content: &str) -> Result<(), MutationError> {
    let document = parse_profile(content).map_err(|error| match error {
        ParseError::DynamicSupportedAssignment(_) => MutationError::ProfileNotEditable(id.clone()),
    })?;
    if !document.editable {
        return Err(MutationError::ProfileNotEditable(id.clone()));
    }
    validate_profile(id, document)
        .map(|_| ())
        .map_err(|ValidationError::InvalidField(field)| MutationError::InvalidField(field))
}

struct OpenCatalog {
    root_path: PathBuf,
    root: cap_std::fs::Dir,
    profiles: cap_std::fs::Dir,
    _lock: std::fs::File,
}

impl OpenCatalog {
    fn profiles_path(&self) -> PathBuf {
        self.root_path.join("profiles")
    }

    fn inspect(&self) -> Result<CatalogInspection, MutationError> {
        let root = self
            .root
            .try_clone()
            .map_err(io_error("open catalog", self.root_path.clone()))?
            .into_std_file();
        Ok(inspect_open_catalog(&self.root_path, &root)?)
    }

    fn profile_name(id: &ProfileId) -> String {
        format!("{}.env", id.as_str())
    }

    fn profile_exists(&self, id: &ProfileId) -> Result<bool, MutationError> {
        let name = Self::profile_name(id);
        match self.profiles.symlink_metadata(&name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(MutationError::Io {
                operation: "inspect profile",
                path: self.profiles_path().join(name),
                source: error,
            }),
        }
    }

    fn refuse_link(&self, dir: &cap_std::fs::Dir, base: &Path, name: &str) -> Result<(), MutationError> {
        match dir.symlink_metadata(name) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(MutationError::UnsafePath(base.join(name)))
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(MutationError::Io {
                operation: "inspect file",
                path: base.join(name),
                source: error,
            }),
        }
    }

    fn read_source(&self, id: &ProfileId) -> Result<String, MutationError> {
        let handle = self
            .profiles
            .try_clone()
            .map_err(io_error("open profiles", self.profiles_path()))?
            .into_std_file();
        read_profile_source(&handle, id).map_err(io_error(
            "read profile",
            self.profiles_path().join(Self::profile_name(id)),
        ))
    }

    fn write_profile(&self, id: &ProfileId, content: &str) -> Result<(), MutationError> {
        let name = Self::profile_name(id);
        let base = self.profiles_path();
        self.refuse_link(&self.profiles, &base, &name)?;
        atomic_write(&self.profiles, &name, content.as_bytes())
            .map_err(io_error("write profile", base.join(&name)))
    }

    fn write_active(&self, id: &ProfileId) -> Result<(), MutationError> {
        self.refuse_link(&self.root, &self.root_path, ACTIVE_FILE)?;
        atomic_write(&self.root, ACTIVE_FILE, format!("{}\n", id.as_str()).as_bytes())
            .map_err(io_error("write active profile", self.root_path.join(ACTIVE_FILE)))
    }

    fn remove_profile(&self, id: &ProfileId) -> std::io::Result<()> {
        retry_sharing_violation(|| self.profiles.remove_file(Self::profile_name(id)))
    }

    fn remove_active(&self) -> std::io::Result<()> {
        match retry_sharing_violation(|| self.root.remove_file(ACTIVE_FILE)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn rename_profile(&self, old: &ProfileId, new: &ProfileId) -> std::io::Result<()> {
        retry_sharing_violation(|| {
            self.profiles.rename(
                Self::profile_name(old),
                &self.profiles,
                Self::profile_name(new),
            )
        })
    }
}

/// Windows refuses to replace or delete a file another process has open
/// without delete sharing, which a concurrent reader can cause briefly.
fn retry_sharing_violation(mut operation: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut attempts = 0;
    loop {
        match operation() {
            Err(error)
                if cfg!(windows)
                    && error.kind() == std::io::ErrorKind::PermissionDenied
                    && attempts < 20 =>
            {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

fn temporary_name(target: &str, attempt: u32) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or_default();
    format!(".{target}.tmp-{}-{nanos:08x}-{attempt}", std::process::id())
}

fn private_file_options() -> cap_std::fs::OpenOptions {
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn atomic_write(dir: &cap_std::fs::Dir, target: &str, contents: &[u8]) -> std::io::Result<()> {
    let mut attempt = 0;
    let (temporary, mut file) = loop {
        let name = temporary_name(target, attempt);
        match dir.open_with(&name, &private_file_options()) {
            Ok(file) => break (name, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 16 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    };
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| retry_sharing_violation(|| dir.rename(&temporary, dir, target)));
    if result.is_err() {
        let _ = dir.remove_file(&temporary);
    }
    result
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Opens the catalog for writing and takes the exclusive mutation lock.
fn open_for_mutation(root_path: &Path, create: bool) -> Result<OpenCatalog, MutationError> {
    if !root_path.is_absolute() {
        return Err(MutationError::UnsafePath(root_path.to_owned()));
    }
    if create {
        create_private_dir(root_path).map_err(io_error("create catalog", root_path.to_owned()))?;
    }
    let root = match cap_std::fs::Dir::open_ambient_dir(root_path, cap_std::ambient_authority()) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MutationError::Store(ProfileStoreError::NotConfigured));
        }
        Err(error) => return Err(io_error("open catalog", root_path.to_owned())(error)),
    };

    let lock_path = root_path.join(LOCK_FILE);
    if let Ok(metadata) = root.symlink_metadata(LOCK_FILE) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(MutationError::UnsafePath(lock_path));
        }
    }
    let mut lock_options = cap_std::fs::OpenOptions::new();
    lock_options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        lock_options.mode(0o600);
    }
    let lock = root
        .open_with(LOCK_FILE, &lock_options)
        .map_err(io_error("open lock", lock_path.clone()))?
        .into_std();
    lock.lock().map_err(io_error("acquire lock", lock_path))?;

    let root_handle = root
        .try_clone()
        .map_err(io_error("open catalog", root_path.to_owned()))?
        .into_std_file();
    let profiles = match open_profiles_dir(root_path, &root_handle)? {
        Some(handle) => cap_std::fs::Dir::from_std_file(handle),
        None if create => {
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use cap_std::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            root.create_dir_with("profiles", &builder)
                .map_err(io_error("create profiles", root_path.join("profiles")))?;
            let handle = open_profiles_dir(root_path, &root_handle)?.ok_or_else(|| {
                MutationError::Store(ProfileStoreError::NotConfigured)
            })?;
            cap_std::fs::Dir::from_std_file(handle)
        }
        None => return Err(MutationError::Store(ProfileStoreError::NotConfigured)),
    };
    Ok(OpenCatalog {
        root_path: root_path.to_owned(),
        root,
        profiles,
        _lock: lock,
    })
}

fn require_existing(
    inspection: &CatalogInspection,
    id: &ProfileId,
) -> Result<CatalogEntryStatus, MutationError> {
    inspection
        .entry(id)
        .map(|entry| entry.status.clone())
        .ok_or_else(|| MutationError::ProfileNotFound(id.clone()))
}

fn require_usable(inspection: &CatalogInspection, id: &ProfileId) -> Result<(), MutationError> {
    match require_existing(inspection, id)? {
        CatalogEntryStatus::Usable(_) => Ok(()),
        CatalogEntryStatus::Unusable(_) => Err(MutationError::ProfileUnusable(id.clone())),
    }
}

/// Creates a new app-written profile, activating it when no usable profile
/// is currently active.
pub fn create_profile(
    root_path: &Path,
    id: &ProfileId,
    fields: &ProfileFields,
) -> Result<CreateOutcome, MutationError> {
    for (field, value) in [
        ("label", &fields.label),
        ("host", &fields.host),
        ("remote_home", &fields.remote_home),
        ("remote_dir", &fields.remote_dir),
    ] {
        if value.is_none() {
            return Err(MutationError::MissingField(field));
        }
    }
    let content = render_new_profile(fields);
    check_rendered(id, &content)?;

    let catalog = open_for_mutation(root_path, true)?;
    if catalog.profile_exists(id)? {
        return Err(MutationError::ProfileExists(id.clone()));
    }
    let inspection = catalog.inspect()?;
    let activate = !inspection.stored_active_is_usable();
    catalog.write_profile(id, &content)?;
    if activate {
        if let Err(error) = catalog.write_active(id) {
            let _ = catalog.remove_profile(id);
            return Err(error);
        }
    }
    Ok(CreateOutcome {
        activated: activate,
    })
}

/// Rewrites supported fields of an app-written profile, preserving other lines.
pub fn update_profile(
    root_path: &Path,
    id: &ProfileId,
    fields: &ProfileFields,
) -> Result<(), MutationError> {
    if fields.is_empty() {
        return Err(MutationError::NoFieldsToUpdate);
    }
    let catalog = open_for_mutation(root_path, false)?;
    let inspection = catalog.inspect()?;
    match require_existing(&inspection, id)? {
        CatalogEntryStatus::Usable(profile) if !profile.editable => {
            return Err(MutationError::ProfileNotEditable(id.clone()));
        }
        CatalogEntryStatus::Unusable(UnusableReason::DynamicSyntax(_)) => {
            return Err(MutationError::ProfileNotEditable(id.clone()));
        }
        CatalogEntryStatus::Unusable(UnusableReason::Unreadable) => {
            return Err(MutationError::ProfileUnusable(id.clone()));
        }
        CatalogEntryStatus::Usable(_)
        | CatalogEntryStatus::Unusable(UnusableReason::InvalidField(_)) => {}
    }
    let original = catalog.read_source(id)?;
    let original_document = parse_profile(&original)
        .map_err(|_| MutationError::ProfileNotEditable(id.clone()))?;
    if !original_document.editable {
        return Err(MutationError::ProfileNotEditable(id.clone()));
    }
    let content = render_updated_profile(&original, fields);
    check_rendered(id, &content)?;
    catalog.write_profile(id, &content)
}

/// Renames a profile file, carrying the active selection with it.
pub fn rename_profile(
    root_path: &Path,
    old: &ProfileId,
    new: &ProfileId,
) -> Result<RenameOutcome, MutationError> {
    let catalog = open_for_mutation(root_path, false)?;
    let inspection = catalog.inspect()?;
    require_existing(&inspection, old)?;
    let profiles_path = catalog.profiles_path();
    catalog.refuse_link(&catalog.profiles, &profiles_path, &OpenCatalog::profile_name(old))?;
    if catalog.profile_exists(new)? {
        return Err(MutationError::ProfileExists(new.clone()));
    }
    let was_active = inspection.stored_active_profile_id.as_ref() == Some(old);
    catalog
        .rename_profile(old, new)
        .map_err(io_error("rename profile", profiles_path.join(OpenCatalog::profile_name(old))))?;
    if was_active {
        if let Err(error) = catalog.write_active(new) {
            let _ = catalog.rename_profile(new, old);
            return Err(error);
        }
    }
    Ok(RenameOutcome {
        active_updated: was_active,
    })
}

/// Makes a usable profile the active destination.
pub fn set_active_profile(root_path: &Path, id: &ProfileId) -> Result<(), MutationError> {
    let catalog = open_for_mutation(root_path, false)?;
    let inspection = catalog.inspect()?;
    require_usable(&inspection, id)?;
    catalog.write_active(id)
}

/// Deletes a profile. Deleting the active profile requires a different
/// usable replacement, and the last usable profile cannot be deleted.
pub fn delete_profile(
    root_path: &Path,
    id: &ProfileId,
    switch_to: Option<&ProfileId>,
) -> Result<DeleteOutcome, MutationError> {
    let catalog = open_for_mutation(root_path, false)?;
    let inspection = catalog.inspect()?;
    let status = require_existing(&inspection, id)?;
    let profiles_path = catalog.profiles_path();
    let name = OpenCatalog::profile_name(id);
    catalog.refuse_link(&catalog.profiles, &profiles_path, &name)?;
    let target_usable = matches!(status, CatalogEntryStatus::Usable(_));
    if target_usable && inspection.usable_count() <= 1 {
        return Err(MutationError::LastUsableProfile);
    }
    let stored_active = inspection.stored_active_profile_id.as_ref() == Some(id);
    let effective_active = inspection.effective_active_profile_id() == Some(id);
    let mut active_profile_id = inspection.effective_active_profile_id().cloned();
    if !(stored_active || effective_active) {
        catalog
            .remove_profile(id)
            .map_err(io_error("delete profile", profiles_path.join(&name)))?;
        return Ok(DeleteOutcome { active_profile_id });
    }
    match switch_to {
        Some(replacement) => {
            if replacement == id {
                return Err(MutationError::SwitchToSameProfile);
            }
            require_usable(&inspection, replacement)?;
            catalog.write_active(replacement)?;
            active_profile_id = Some(replacement.clone());
            catalog
                .remove_profile(id)
                .map_err(|source| MutationError::PartiallyApplied {
                    operation: "delete profile",
                    path: profiles_path.join(&name),
                    source,
                })?;
        }
        // A broken profile that is merely named by the active-profile file
        // can go; the selection falls back to the first usable profile.
        None if !target_usable => {
            catalog
                .remove_profile(id)
                .map_err(io_error("delete profile", profiles_path.join(&name)))?;
            catalog
                .remove_active()
                .map_err(|source| MutationError::PartiallyApplied {
                    operation: "clear active profile",
                    path: catalog.root_path.join(ACTIVE_FILE),
                    source,
                })?;
        }
        None => return Err(MutationError::SwitchToRequired),
    }
    Ok(DeleteOutcome { active_profile_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_round_trips_through_the_literal_parser() {
        let label = r#"Ops "west" $HOME `x` \ end"#;
        let fields = ProfileFields {
            label: Some(label.into()),
            host: Some("ops@example.test".into()),
            remote_home: Some("/home/ops".into()),
            remote_dir: Some("img-uploads".into()),
            ..ProfileFields::default()
        };
        let document = parse_profile(&render_new_profile(&fields)).expect("parse");
        assert!(document.editable);
        assert_eq!(document.label.as_deref(), Some(label));
    }

    #[test]
    fn update_preserves_unrelated_lines_and_appends_missing_fields() {
        let original = "# note\nSSH_PROFILE_LABEL='Old'\nCUSTOM=1\nSSH_HOST=old\n";
        let fields = ProfileFields {
            label: Some("New".into()),
            restore_seconds: Some("30".into()),
            ..ProfileFields::default()
        };
        assert_eq!(
            render_updated_profile(original, &fields),
            "# note\nSSH_PROFILE_LABEL=\"New\"\nCUSTOM=1\nSSH_HOST=old\nSSH_CLIP_RESTORE_SECONDS=\"30\"\n"
        );
    }
}
