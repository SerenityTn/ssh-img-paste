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

pub fn resolve_profile(
    root_path: &Path,
    selection: ProfileSelection,
) -> Result<SelectedProfile, ProfileStoreError> {
    let root = open_catalog_root(root_path)?;
    let profiles = discover_profile_entries(root_path, &root)?;
    if profiles.is_empty() {
        return Err(ProfileStoreError::NotConfigured);
    }
    let requested_id = match selection {
        ProfileSelection::Active => read_active_profile_id(root_path, &root)?,
        ProfileSelection::Explicit(id) => Some(id),
    };
    let discovered = match requested_id {
        Some(id) => profiles
            .iter()
            .find(|profile| profile.id == id)
            .ok_or(ProfileStoreError::ProfileNotFound(id))?,
        None => profiles.first().ok_or(ProfileStoreError::NotConfigured)?,
    };
    Ok(SelectedProfile {
        id: discovered.id.clone(),
        profile: discovered.profile.clone(),
    })
}

pub fn discover_profiles(root_path: &Path) -> Result<ProfileCatalog, ProfileStoreError> {
    let root = open_catalog_root(root_path)?;
    Ok(ProfileCatalog {
        active_profile_id: read_active_profile_id(root_path, &root)?,
        profiles: discover_profile_entries(root_path, &root)?,
    })
}
