use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRootError {
    MissingEnvironment(&'static str),
    PathMustBeAbsolute(&'static str),
}

fn environment_path(
    environment: &BTreeMap<String, OsString>,
    name: &'static str,
) -> Result<Option<PathBuf>, ConfigRootError> {
    let Some(value) = environment.get(name) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    if !path.is_absolute() {
        return Err(ConfigRootError::PathMustBeAbsolute(name));
    }
    Ok(Some(path))
}

pub fn default_config_root(
    override_path: Option<&Path>,
    environment: &BTreeMap<String, OsString>,
) -> Result<PathBuf, ConfigRootError> {
    if let Some(path) = override_path {
        if !path.is_absolute() {
            return Err(ConfigRootError::PathMustBeAbsolute("--config-root"));
        }
        return Ok(path.to_owned());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = environment_path(environment, "LOCALAPPDATA")? {
            return Ok(path.join("SSH Image Paste"));
        }
        if let Some(path) = environment_path(environment, "USERPROFILE")? {
            return Ok(path.join("AppData").join("Local").join("SSH Image Paste"));
        }
        Err(ConfigRootError::MissingEnvironment(
            "LOCALAPPDATA or USERPROFILE",
        ))
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(path) = environment_path(environment, "XDG_CONFIG_HOME")? {
            return Ok(path.join("ssh-img-paste"));
        }
        if let Some(path) = environment_path(environment, "HOME")? {
            return Ok(path.join(".config").join("ssh-img-paste"));
        }
        Err(ConfigRootError::MissingEnvironment(
            "XDG_CONFIG_HOME or HOME",
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = environment;
        Err(ConfigRootError::MissingEnvironment("supported platform"))
    }
}

pub fn current_environment() -> BTreeMap<String, std::ffi::OsString> {
    ["LOCALAPPDATA", "USERPROFILE", "XDG_CONFIG_HOME", "HOME"]
        .into_iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name.to_owned(), value)))
        .collect()
}
