//! Client-side mirror of the core's profile rules for immediate form
//! feedback, matching the Windows edition. The CLI re-validates everything
//! and remains authoritative.

use unicode_normalization::UnicodeNormalization;

const SHELL_META: &str = "'\"`$;&|<>(){}[]*?\\";
pub const MAX_RESTORE_SECONDS: u32 = 86_400;
pub const DEFAULT_REMOTE_DIR: &str = "img-uploads";

pub const ID_RULE: &str = "Start with a letter or digit, then use letters, digits, - or _.";

pub fn is_valid_id(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub fn validate_id(value: &str, existing: &[&str]) -> Option<&'static str> {
    if !is_valid_id(value) {
        Some(ID_RULE)
    } else if existing.iter().any(|id| id.eq_ignore_ascii_case(value)) {
        Some("A destination with this ID already exists.")
    } else {
        None
    }
}

fn is_control(c: char) -> bool {
    c <= '\u{1f}' || c == '\u{7f}'
}

pub fn validate_label(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("Enter a display name.")
    } else if value.chars().any(is_control) {
        Some("Remove line breaks and control characters.")
    } else {
        None
    }
}

pub fn validate_host(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("Enter an SSH host or alias.")
    } else if value.starts_with('-') {
        Some("The host cannot start with a dash.")
    } else if value.contains(':') {
        Some("Put ports in your SSH config instead of the host field.")
    } else if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'))
    {
        None
    } else {
        Some("Use letters, digits, dots, dashes, underscores, and @ only.")
    }
}

const PATH_RULE: &str = "Use letters, digits, dots, dashes, underscores, and / only.";

pub fn validate_remote_home(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("Enter the absolute home directory on the host.")
    } else if !value.starts_with('/') {
        Some("Use an absolute Linux path such as /home/me.")
    } else if is_safe_path(value) {
        None
    } else {
        Some(PATH_RULE)
    }
}

pub fn validate_remote_dir(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("Enter an upload folder such as img-uploads.")
    } else if value.starts_with('/') {
        Some("Use a folder relative to the remote home.")
    } else if value.starts_with('-') || value == "." {
        Some("Choose a different folder name.")
    } else if is_safe_path(value) {
        None
    } else {
        Some(PATH_RULE)
    }
}

pub fn validate_restore_seconds(value: &str) -> Option<&'static str> {
    let valid = !value.is_empty()
        && value.chars().all(|c| c.is_ascii_digit())
        && value
            .parse::<u32>()
            .is_ok_and(|seconds| seconds <= MAX_RESTORE_SECONDS);
    (!valid).then_some("Use a whole number of seconds from 0 to 86400.")
}

fn is_safe_path(value: &str) -> bool {
    !value.contains("//")
        && !value.split('/').any(|part| part == "." || part == "..")
        && !value
            .chars()
            .any(|c| is_control(c) || SHELL_META.contains(c))
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

/// Derives a profile ID from a display name: accents folded, lowercase,
/// separators collapsed to `-`, at most 40 characters, unique (`-2`, `-3`…).
pub fn suggest_id(label: &str, existing: &[&str]) -> String {
    let mut id = String::new();
    for character in label.nfd() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if (character.is_whitespace() || matches!(character, '-' | '_' | '.'))
            && !id.is_empty()
            && !id.ends_with('-')
        {
            id.push('-');
        }
    }
    let mut base = id.trim_matches('-').to_owned();
    if base.len() > 40 {
        base = base[..40].trim_matches('-').to_owned();
    }
    if base.is_empty() {
        base = "destination".into();
    }
    let taken = |candidate: &str| existing.iter().any(|id| id.eq_ignore_ascii_case(candidate));
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| !taken(candidate))
        .expect("an unused suffix exists")
}

/// `n B`, `x.y KB`, or `x.y MB`, base 1024.
pub fn format_size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes < 1024.0 {
        format!("{bytes} B")
    } else if bytes < 1024.0 * 1024.0 {
        format!("{:.1} KB", bytes / 1024.0)
    } else {
        format!("{:.1} MB", bytes / (1024.0 * 1024.0))
    }
}

pub fn plural(count: i64, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_the_core_pattern() {
        assert!(is_valid_id("work-vps_2"));
        assert!(!is_valid_id("-work"));
        assert!(!is_valid_id("work vps"));
        assert!(!is_valid_id(""));
        assert_eq!(
            validate_id("Work", &["work"]),
            Some("A destination with this ID already exists.")
        );
    }

    #[test]
    fn suggestions_fold_accents_and_stay_unique() {
        assert_eq!(suggest_id("Café Server", &[]), "cafe-server");
        assert_eq!(suggest_id("  My.Work__VPS  ", &[]), "my-work-vps");
        assert_eq!(suggest_id("!!!", &[]), "destination");
        assert_eq!(suggest_id("Work", &["work", "WORK-2"]), "work-3");
        assert_eq!(suggest_id(&"a".repeat(50), &[]).len(), 40);
    }

    #[test]
    fn host_rules_match_the_windows_edition() {
        assert_eq!(validate_host("me@work.example"), None);
        assert_eq!(validate_host(""), Some("Enter an SSH host or alias."));
        assert_eq!(
            validate_host("-oProxy"),
            Some("The host cannot start with a dash.")
        );
        assert_eq!(
            validate_host("work:2222"),
            Some("Put ports in your SSH config instead of the host field.")
        );
        assert!(validate_host("work vps").is_some());
    }

    #[test]
    fn path_rules_reject_traversal_and_shell_syntax() {
        assert_eq!(validate_remote_home("/home/me"), None);
        assert!(validate_remote_home("home/me").is_some());
        assert!(validate_remote_home("/home/../root").is_some());
        assert!(validate_remote_home("/home//me").is_some());
        assert_eq!(validate_remote_dir("img-uploads/2026"), None);
        assert!(validate_remote_dir("/abs").is_some());
        assert!(validate_remote_dir(".").is_some());
        assert!(validate_remote_dir("$HOME").is_some());
    }

    #[test]
    fn restore_seconds_are_whole_numbers_in_range() {
        assert_eq!(validate_restore_seconds("0"), None);
        assert_eq!(validate_restore_seconds("86400"), None);
        assert!(validate_restore_seconds("86401").is_some());
        assert!(validate_restore_seconds("-1").is_some());
        assert!(validate_restore_seconds(" 5").is_some());
    }

    #[test]
    fn labels_reject_control_characters() {
        assert_eq!(validate_label("Work"), None);
        assert!(validate_label("").is_some());
        assert!(validate_label("a\nb").is_some());
    }

    #[test]
    fn sizes_use_base_1024() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
        assert_eq!(plural(1, "image", "images"), "1 image");
        assert_eq!(plural(2, "image", "images"), "2 images");
    }
}
