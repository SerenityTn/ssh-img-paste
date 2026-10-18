use crate::{
    ParseError, ProfileId, ValidatedProfile, ValidationError, parse_profile, validate_profile,
};
use std::path::{Path, PathBuf};

fn open_catalog_root(root: &Path) -> Result<std::fs::File, ProfileStoreError> {
    cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map(cap_std::fs::Dir::into_std_file)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProfileStoreError::NotConfigured
            } else {
                ProfileStoreError::Io {
                    path: root.to_owned(),
                    source,
                }
            }
        })
}

fn open_relative_file(start: &std::fs::File, path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = cap_primitives::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_primitives::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use cap_primitives::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = cap_primitives::fs::open(start, path, &options)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "profile file is a reparse point",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "profile path is not a regular file",
        ));
    }
    Ok(file)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredProfile {
    pub id: ProfileId,
    pub profile: ValidatedProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCatalog {
    pub active_profile_id: Option<ProfileId>,
    pub profiles: Vec<DiscoveredProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSelection {
    Active,
    Explicit(ProfileId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedProfile {
    pub id: ProfileId,
    pub profile: ValidatedProfile,
}

#[derive(Debug)]
pub enum ProfileStoreError {
    NotConfigured,
    ProfileNotFound(ProfileId),
    InvalidActiveProfile,
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        id: ProfileId,
        source: ParseError,
    },
    Validation {
        id: ProfileId,
        source: ValidationError,
    },
}

fn read_active_profile_id(
    root_path: &Path,
    root: &std::fs::File,
) -> Result<Option<ProfileId>, ProfileStoreError> {
    let display_path = root_path.join("active-profile");
    let mut file = match open_relative_file(root, Path::new("active-profile")) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ProfileStoreError::Io {
                path: display_path,
                source,
            });
        }
    };
    let mut input = String::new();
    std::io::Read::read_to_string(&mut file, &mut input).map_err(|source| {
        ProfileStoreError::Io {
            path: display_path,
            source,
        }
    })?;
    let value = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(&input);
    if value.is_empty() || value.contains(['\r', '\n']) {
        return Err(ProfileStoreError::InvalidActiveProfile);
    }
    ProfileId::parse(value)
        .map(Some)
        .map_err(|_| ProfileStoreError::InvalidActiveProfile)
}

fn discover_profile_entries(
    root_path: &Path,
    root: &std::fs::File,
) -> Result<Vec<DiscoveredProfile>, ProfileStoreError> {
    let profiles_path = root_path.join("profiles");
    let profiles_handle = cap_primitives::fs::open_dir_nofollow(root, Path::new("profiles"))
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProfileStoreError::NotConfigured
            } else {
                ProfileStoreError::Io {
                    path: profiles_path.clone(),
                    source,
                }
            }
        })?;
    let profiles_dir = cap_std::fs::Dir::from_std_file(profiles_handle);
    let entries = profiles_dir
        .entries()
        .map_err(|source| ProfileStoreError::Io {
            path: profiles_path.clone(),
            source,
        })?;

    let mut profile_files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProfileStoreError::Io {
            path: profiles_path.clone(),
            source,
        })?;
        let name = entry.file_name();
        let path = Path::new(&name);
        if path.extension().and_then(|value| value.to_str()) != Some("env") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(id) = ProfileId::parse(stem) else {
            continue;
        };
        profile_files.push((id, name));
    }
    profile_files.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));

    let profiles_handle = profiles_dir.into_std_file();
    let mut profiles = Vec::with_capacity(profile_files.len());
    for (id, name) in profile_files {
        let display_path = profiles_path.join(&name);
        let mut file =
            open_relative_file(&profiles_handle, Path::new(&name)).map_err(|source| {
                ProfileStoreError::Io {
                    path: display_path.clone(),
                    source,
                }
            })?;
        let mut input = String::new();
        std::io::Read::read_to_string(&mut file, &mut input).map_err(|source| {
            ProfileStoreError::Io {
                path: display_path,
                source,
            }
        })?;
        let document = parse_profile(&input).map_err(|source| ProfileStoreError::Parse {
            id: id.clone(),
            source,
        })?;
        let profile =
            validate_profile(&id, document).map_err(|source| ProfileStoreError::Validation {
                id: id.clone(),
                source,
            })?;
        profiles.push(DiscoveredProfile { id, profile });
    }
    Ok(profiles)
}

/// Resolves one profile for an operation.
///
/// Only the selected profile has to be usable: a broken, unrelated profile
/// file never blocks uploads to a healthy destination. Unsafe catalog
/// directories still fail closed.
pub fn resolve_profile(
    root_path: &Path,
    selection: ProfileSelection,
) -> Result<SelectedProfile, ProfileStoreError> {
    let root = open_catalog_root(root_path)?;
    let inspection = inspect_open_catalog(root_path, &root)?;
    if !inspection.configured || inspection.entries.is_empty() {
        return Err(ProfileStoreError::NotConfigured);
    }
    let entry = match selection {
        ProfileSelection::Explicit(id) => inspection
            .entry(&id)
            .ok_or(ProfileStoreError::ProfileNotFound(id))?,
        ProfileSelection::Active => {
            if inspection.active_state_invalid {
                return Err(ProfileStoreError::InvalidActiveProfile);
            }
            match &inspection.stored_active_profile_id {
                Some(id) => inspection
                    .entry(id)
                    .ok_or_else(|| ProfileStoreError::ProfileNotFound(id.clone()))?,
                None => inspection
                    .entries
                    .iter()
                    .find(|entry| entry.usable().is_some())
                    .ok_or(ProfileStoreError::NotConfigured)?,
            }
        }
    };
    match &entry.status {
        CatalogEntryStatus::Usable(profile) => Ok(SelectedProfile {
            id: entry.id.clone(),
            profile: profile.clone(),
        }),
        CatalogEntryStatus::Unusable(UnusableReason::DynamicSyntax(key)) => {
            Err(ProfileStoreError::Parse {
                id: entry.id.clone(),
                source: ParseError::DynamicSupportedAssignment(key.clone()),
            })
        }
        CatalogEntryStatus::Unusable(UnusableReason::InvalidField(field)) => {
            Err(ProfileStoreError::Validation {
                id: entry.id.clone(),
                source: ValidationError::InvalidField(field),
            })
        }
        CatalogEntryStatus::Unusable(UnusableReason::Unreadable) => Err(ProfileStoreError::Io {
            path: root_path
                .join("profiles")
                .join(format!("{}.env", entry.id.as_str())),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "profile file is linked, not regular, or unreadable",
            ),
        }),
    }
}

pub fn discover_profiles(root_path: &Path) -> Result<ProfileCatalog, ProfileStoreError> {
    let root = open_catalog_root(root_path)?;
    Ok(ProfileCatalog {
        active_profile_id: read_active_profile_id(root_path, &root)?,
        profiles: discover_profile_entries(root_path, &root)?,
    })
}

/// Why a profile file cannot be used, without failing the whole catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnusableReason {
    /// A supported key uses dynamic shell syntax that is never evaluated.
    DynamicSyntax(String),
    /// A field failed validation.
    InvalidField(&'static str),
    /// The file is a link, a reparse point, not a regular file, or unreadable.
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogEntryStatus {
    Usable(ValidatedProfile),
    Unusable(UnusableReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: ProfileId,
    pub status: CatalogEntryStatus,
}

impl CatalogEntry {
    pub fn usable(&self) -> Option<&ValidatedProfile> {
        match &self.status {
            CatalogEntryStatus::Usable(profile) => Some(profile),
            CatalogEntryStatus::Unusable(_) => None,
        }
    }
}

/// A fault-tolerant view of the catalog for management surfaces.
///
/// Individual broken profile files are reported rather than failing the
/// whole view. Unsafe catalog directories still fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogInspection {
    /// False when the catalog root or its `profiles` directory is absent.
    pub configured: bool,
    /// The stored active ID, when the active-profile file is well formed.
    pub stored_active_profile_id: Option<ProfileId>,
    /// True when an active-profile file exists but is malformed.
    pub active_state_invalid: bool,
    pub entries: Vec<CatalogEntry>,
}

impl CatalogInspection {
    pub fn entry(&self, id: &ProfileId) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| &entry.id == id)
    }

    pub fn usable_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.usable().is_some())
            .count()
    }

    /// The profile uploads use when no profile is named explicitly.
    ///
    /// This is the stored active profile when it names a usable entry, and
    /// otherwise the first usable entry in ID order.
    pub fn effective_active_profile_id(&self) -> Option<&ProfileId> {
        if let Some(id) = &self.stored_active_profile_id {
            if self.entry(id).and_then(CatalogEntry::usable).is_some() {
                return Some(id);
            }
        }
        self.entries
            .iter()
            .find(|entry| entry.usable().is_some())
            .map(|entry| &entry.id)
    }

    /// True when the stored active ID names a usable profile.
    pub fn stored_active_is_usable(&self) -> bool {
        self.stored_active_profile_id
            .as_ref()
            .and_then(|id| self.entry(id))
            .and_then(CatalogEntry::usable)
            .is_some()
    }
}

pub(crate) fn read_profile_source(
    profiles: &std::fs::File,
    id: &ProfileId,
) -> std::io::Result<String> {
    let name = format!("{}.env", id.as_str());
    let mut file = open_relative_file(profiles, Path::new(&name))?;
    let mut input = String::new();
    std::io::Read::read_to_string(&mut file, &mut input)?;
    Ok(input)
}

pub(crate) fn open_profiles_dir(
    root_path: &Path,
    root: &std::fs::File,
) -> Result<Option<std::fs::File>, ProfileStoreError> {
    match cap_primitives::fs::open_dir_nofollow(root, Path::new("profiles")) {
        Ok(handle) => Ok(Some(handle)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ProfileStoreError::Io {
            path: root_path.join("profiles"),
            source,
        }),
    }
}

pub(crate) fn inspect_open_catalog(
    root_path: &Path,
    root: &std::fs::File,
) -> Result<CatalogInspection, ProfileStoreError> {
    let (stored_active_profile_id, active_state_invalid) =
        match read_active_profile_id(root_path, root) {
            Ok(id) => (id, false),
            Err(ProfileStoreError::InvalidActiveProfile) => (None, true),
            Err(error) => return Err(error),
        };
    let Some(profiles_handle) = open_profiles_dir(root_path, root)? else {
        return Ok(CatalogInspection {
            configured: false,
            stored_active_profile_id,
            active_state_invalid,
            entries: Vec::new(),
        });
    };
    let profiles_path = root_path.join("profiles");
    let profiles_dir = cap_std::fs::Dir::from_std_file(profiles_handle);
    let entries = profiles_dir
        .entries()
        .map_err(|source| ProfileStoreError::Io {
            path: profiles_path.clone(),
            source,
        })?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProfileStoreError::Io {
            path: profiles_path.clone(),
            source,
        })?;
        let name = entry.file_name();
        let path = Path::new(&name);
        if path.extension().and_then(|value| value.to_str()) != Some("env") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Ok(id) = ProfileId::parse(stem) {
            ids.push(id);
        }
    }
    ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let profiles_handle = profiles_dir.into_std_file();
    let entries = ids
        .into_iter()
        .map(|id| {
            let status = match read_profile_source(&profiles_handle, &id) {
                Err(_) => CatalogEntryStatus::Unusable(UnusableReason::Unreadable),
                Ok(input) => match parse_profile(&input) {
                    Err(ParseError::DynamicSupportedAssignment(key)) => {
                        CatalogEntryStatus::Unusable(UnusableReason::DynamicSyntax(key))
                    }
                    Ok(document) => match validate_profile(&id, document) {
                        Ok(profile) => CatalogEntryStatus::Usable(profile),
                        Err(ValidationError::InvalidField(field)) => {
                            CatalogEntryStatus::Unusable(UnusableReason::InvalidField(field))
                        }
                    },
                },
            };
            CatalogEntry { id, status }
        })
        .collect();
    Ok(CatalogInspection {
        configured: true,
        stored_active_profile_id,
        active_state_invalid,
        entries,
    })
}

/// Inspects the catalog without failing on individual broken profiles.
pub fn inspect_catalog(root_path: &Path) -> Result<CatalogInspection, ProfileStoreError> {
    let root = match open_catalog_root(root_path) {
        Ok(root) => root,
        Err(ProfileStoreError::NotConfigured) => return Ok(CatalogInspection::default()),
        Err(error) => return Err(error),
    };
    inspect_open_catalog(root_path, &root)
}
