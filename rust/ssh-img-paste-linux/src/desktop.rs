//! Toolkit-free desktop model: the tray menu (mirroring the Windows
//! edition's `TrayMenuModel`), tray tooltips, and the XDG autostart entry.

use crate::cli::Catalog;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "io.github.serenitytn.SSHImagePaste";
pub const PRODUCT_NAME: &str = "SSH Image Paste";
const MAX_TOOLTIP_CHARS: usize = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    OpenWindow,
    UploadClipboard,
    CaptureRegion,
    CaptureFullScreen,
    UploadedImages,
    ManageProfiles,
    Quit,
}

pub const PRIMARY_ACTIONS: [DesktopAction; 7] = [
    DesktopAction::OpenWindow,
    DesktopAction::UploadClipboard,
    DesktopAction::CaptureRegion,
    DesktopAction::CaptureFullScreen,
    DesktopAction::UploadedImages,
    DesktopAction::ManageProfiles,
    DesktopAction::Quit,
];

pub const DESTINATION_MENU_LABEL: &str = "Destination";
pub const NO_DESTINATIONS_LABEL: &str = "No destinations yet";

impl DesktopAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenWindow => "Open SSH Image Paste",
            Self::UploadClipboard => "Upload Clipboard Image",
            Self::CaptureRegion => "Capture Region",
            Self::CaptureFullScreen => "Capture Full Screen",
            Self::UploadedImages => "Uploaded Images…",
            Self::ManageProfiles => "Manage Profiles…",
            Self::Quit => "Quit",
        }
    }

    /// Actions preceded by a separator in the tray menu.
    pub fn starts_group(self) -> bool {
        matches!(
            self,
            Self::UploadClipboard | Self::UploadedImages | Self::Quit
        )
    }

    /// Actions that need a usable active destination.
    pub fn requires_destination(self) -> bool {
        matches!(
            self,
            Self::UploadClipboard
                | Self::CaptureRegion
                | Self::CaptureFullScreen
                | Self::UploadedImages
        )
    }

    /// Actions that start an upload, so they wait for the current one.
    pub fn uploads(self) -> bool {
        matches!(
            self,
            Self::UploadClipboard | Self::CaptureRegion | Self::CaptureFullScreen
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indicator {
    #[default]
    Idle,
    Busy,
    Success,
    Error,
}

impl Indicator {
    /// Themed icon for transient states; idle uses the app icon.
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Idle => APP_ID,
            Self::Busy => "emblem-synchronizing-symbolic",
            Self::Success => "emblem-ok-symbolic",
            Self::Error => "dialog-error-symbolic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationItem {
    pub id: String,
    /// `Label  (host)`.
    pub label: String,
}

/// Everything the tray shows; rebuilt from the catalog and upload state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    pub destinations: Vec<DestinationItem>,
    pub active_id: Option<String>,
    pub active_label: Option<String>,
    pub busy: bool,
    pub indicator: Indicator,
    /// Replaces the idle tooltip while a busy, success or error state shows.
    pub detail: Option<String>,
}

impl TrayState {
    pub fn from_catalog(catalog: Option<&Catalog>) -> Self {
        let Some(catalog) = catalog else {
            return Self::default();
        };
        let active = catalog.active();
        Self {
            destinations: catalog
                .usable()
                .map(|profile| DestinationItem {
                    id: profile.id.clone(),
                    label: profile.menu_label(),
                })
                .collect(),
            active_id: active.map(|profile| profile.id.clone()),
            active_label: active.map(|profile| profile.label.clone()),
            ..Self::default()
        }
    }

    pub fn has_destination(&self) -> bool {
        self.active_id.is_some()
    }

    pub fn enabled(&self, action: DesktopAction) -> bool {
        if action.uploads() && self.busy {
            return false;
        }
        !action.requires_destination() || self.has_destination()
    }

    /// Index of the active destination in the radio group, if listed.
    pub fn selected_destination(&self) -> Option<usize> {
        let active = self.active_id.as_deref()?;
        self.destinations.iter().position(|item| item.id == active)
    }

    pub fn tooltip(&self) -> String {
        let text = match (&self.detail, self.indicator) {
            (Some(detail), Indicator::Busy | Indicator::Success | Indicator::Error) => {
                detail.clone()
            }
            _ => match &self.active_label {
                Some(label) => format!("{PRODUCT_NAME} — {label}"),
                None => format!("{PRODUCT_NAME} — no destination"),
            },
        };
        truncate_chars(&text, MAX_TOOLTIP_CHARS)
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    match value.char_indices().nth(limit) {
        Some((cut, _)) => value[..cut].to_owned(),
        None => value.to_owned(),
    }
}

/// `$XDG_CONFIG_HOME/autostart/<app id>.desktop`, from absolute paths only.
pub fn autostart_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name: &str| {
        env(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let config = absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".config")))?;
    Some(config.join("autostart").join(format!("{APP_ID}.desktop")))
}

/// The autostart entry; the app starts in the background, tray only.
pub fn autostart_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={PRODUCT_NAME}\nComment=Upload screenshots over SSH and copy the remote path\n\
         Exec={} --background\nIcon={APP_ID}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_quote(&executable.to_string_lossy())
    )
}

/// Quotes one `Exec` argument per the Desktop Entry specification.
pub fn desktop_exec_quote(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+' | ','));
    if plain {
        return argument.to_owned();
    }
    let mut quoted = String::from("\"");
    for character in argument.chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    // `%` is a field code even inside quotes.
    quoted.replace('%', "%%")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Profile, ShotMode};

    fn profile(id: &str, label: &str, usable: bool) -> Profile {
        Profile {
            id: id.into(),
            usable,
            active: false,
            label: label.into(),
            host: format!("{id}-ssh"),
            remote_home: "/home/me".into(),
            remote_dir: "img-uploads".into(),
            shot_mode: ShotMode::Region,
            restore_seconds: 60,
            editable: true,
            problem: None,
        }
    }

    fn catalog(active: Option<&str>) -> Catalog {
        Catalog {
            configured: true,
            config_root: "/home/me/.config/ssh-img-paste".into(),
            active_profile_id: active.map(str::to_owned),
            profiles: vec![
                profile("work", "Work", true),
                profile("broken", "broken", false),
                profile("home", "Home", true),
            ],
        }
    }

    #[test]
    fn menu_matches_the_windows_edition() {
        let labels: Vec<_> = PRIMARY_ACTIONS
            .iter()
            .map(|action| action.label())
            .collect();
        assert_eq!(
            labels,
            [
                "Open SSH Image Paste",
                "Upload Clipboard Image",
                "Capture Region",
                "Capture Full Screen",
                "Uploaded Images…",
                "Manage Profiles…",
                "Quit"
            ]
        );
        let groups: Vec<_> = PRIMARY_ACTIONS
            .iter()
            .filter(|action| action.starts_group())
            .map(|action| action.label())
            .collect();
        assert_eq!(
            groups,
            ["Upload Clipboard Image", "Uploaded Images…", "Quit"]
        );
    }

    #[test]
    fn destinations_list_usable_profiles_with_the_active_one_selected() {
        let state = TrayState::from_catalog(Some(&catalog(Some("home"))));
        let labels: Vec<_> = state
            .destinations
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["Work  (work-ssh)", "Home  (home-ssh)"]);
        assert_eq!(state.selected_destination(), Some(1));
        assert_eq!(state.tooltip(), "SSH Image Paste — Home");
    }

    #[test]
    fn actions_need_a_destination_and_wait_for_uploads() {
        let none = TrayState::from_catalog(Some(&catalog(None)));
        assert!(!none.enabled(DesktopAction::UploadClipboard));
        assert!(!none.enabled(DesktopAction::UploadedImages));
        assert!(none.enabled(DesktopAction::ManageProfiles));
        assert_eq!(none.tooltip(), "SSH Image Paste — no destination");

        let mut busy = TrayState::from_catalog(Some(&catalog(Some("work"))));
        busy.busy = true;
        assert!(!busy.enabled(DesktopAction::CaptureRegion));
        assert!(busy.enabled(DesktopAction::UploadedImages));
        assert!(busy.enabled(DesktopAction::Quit));
    }

    #[test]
    fn transient_tooltips_are_truncated() {
        let state = TrayState {
            indicator: Indicator::Error,
            detail: Some("x".repeat(300)),
            ..TrayState::default()
        };
        assert_eq!(state.tooltip().chars().count(), 127);
    }

    #[test]
    fn autostart_entry_quotes_the_executable() {
        let entry = autostart_entry(Path::new("/opt/SSH Image Paste/ssh-img-paste-linux"));
        assert!(entry.contains("Exec=\"/opt/SSH Image Paste/ssh-img-paste-linux\" --background\n"));
        assert_eq!(desktop_exec_quote("/usr/bin/app"), "/usr/bin/app");
        assert_eq!(desktop_exec_quote("/a$b"), "\"/a\\$b\"");
        assert_eq!(desktop_exec_quote("/100%"), "\"/100%%\"");
        let path = autostart_path(|name| (name == "HOME").then(|| OsString::from("/home/me")));
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/me/.config/autostart/io.github.serenitytn.SSHImagePaste.desktop"
            ))
        );
    }
}
