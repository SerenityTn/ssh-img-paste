//! Turns protocol errors into short, actionable messages for Linux users.
//! Mirrors the Windows edition's `ErrorMessages` with Linux-specific hints.

use crate::cli::CliError;

pub fn field_label(field: Option<&str>) -> &'static str {
    match field {
        Some("label") => "Display name",
        Some("host") => "SSH host or alias",
        Some("remote_home") => "Remote home",
        Some("remote_dir") => "Upload folder",
        Some("shot_mode") => "Screenshot fallback",
        Some("restore_seconds") => "Clipboard restore delay",
        _ => "A field",
    }
}

pub fn describe(error: &CliError) -> String {
    match error.code.as_str() {
        "connection_failed" | "upload_failed" | "remote_command_failed" => describe_remote(error),
        "invalid_field" | "profile_validation_failed" => {
            format!("{} is invalid.", field_label(error.field.as_deref()))
        }
        "missing_field" => format!("{} is required.", field_label(error.field.as_deref())),
        "profile_not_found" => "That destination no longer exists. Choose another one.".into(),
        "profile_unusable" => "That destination is invalid. Fix or delete it first.".into(),
        "profile_parse_failed" => "That destination uses shell syntax such as $VARIABLES, which \
                                   is never evaluated. Use literal values."
            .into(),
        "not_configured" => "No destination is set up yet. Add one with \
                             \"ssh-img-paste profiles create\"."
            .into(),
        "invalid_active_profile" => {
            "The active destination setting is damaged. Choose a destination again.".into()
        }
        "profile_store_unsafe" => {
            "The settings folder contains a link or unexpected file, so nothing was changed.".into()
        }
        "remote_output_invalid" => "The host returned an unexpected response.".into(),
        "cli_timeout" => {
            "The operation took too long and was stopped. Check the network and try again.".into()
        }
        "source_not_png" => "The image could not be prepared as a PNG.".into(),
        _ => error.message.clone(),
    }
}

fn describe_remote(error: &CliError) -> String {
    let diagnostic = error.diagnostic.as_deref().unwrap_or_default().trim();
    if let Some(hint) = ssh_hint(diagnostic, error) {
        return hint.into();
    }
    if diagnostic.is_empty() {
        error.message.clone()
    } else {
        format!("{} {}", error.message, truncate(diagnostic, 240))
    }
}

/// Recognizes the OpenSSH failures users hit most often.
pub fn ssh_hint(diagnostic: &str, error: &CliError) -> Option<&'static str> {
    let lower = diagnostic.to_ascii_lowercase();
    let has = |text: &str| lower.contains(&text.to_ascii_lowercase());

    if error.reason.as_deref() == Some("spawn") {
        return Some(
            "OpenSSH was not found. Install the OpenSSH client (for example \
             \"sudo apt install openssh-client\").",
        );
    }
    if error.reason.as_deref() == Some("timeout") {
        return Some("The SSH host did not respond in time. Check the network or VPN.");
    }
    if has("Host key verification failed") {
        return Some(
            "The host key is not trusted yet. Run \"ssh <host>\" once in a terminal and accept \
             the key, then retry.",
        );
    }
    if has("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return Some(
            "The host key changed. Verify the server, then remove the old key with \
             \"ssh-keygen -R <host>\".",
        );
    }
    if has("Permission denied") {
        return Some(
            "The SSH key was rejected. Add your key to ssh-agent (\"ssh-add\") or set \
             IdentityFile in ~/.ssh/config. Passphrase prompts are not supported.",
        );
    }
    if has("Could not resolve hostname") {
        return Some("The host name could not be resolved. Check the alias in ~/.ssh/config.");
    }
    if has("Connection refused") {
        return Some("The host refused the SSH connection. Check the port in ~/.ssh/config.");
    }
    if has("timed out") {
        return Some("The SSH connection timed out. Check the network or VPN.");
    }
    None
}

fn truncate(value: &str, length: usize) -> String {
    match value.char_indices().nth(length) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(diagnostic: &str) -> CliError {
        CliError {
            diagnostic: Some(diagnostic.into()),
            step: Some("upload".into()),
            reason: Some("exit".into()),
            ..CliError::new("upload_failed", "The upload failed.")
        }
    }

    #[test]
    fn recognizes_common_ssh_failures() {
        assert!(describe(&remote("user@host: Permission denied (publickey).")).contains("ssh-add"));
        assert!(describe(&remote("Host key verification failed.")).contains("accept the key"));
        assert!(
            describe(&remote(
                "ssh: Could not resolve hostname nope: Name or service not known"
            ))
            .contains("~/.ssh/config")
        );
    }

    #[test]
    fn missing_openssh_suggests_the_package() {
        let error = CliError {
            reason: Some("spawn".into()),
            ..CliError::new("connection_failed", "Could not start ssh.")
        };
        assert!(describe(&error).contains("openssh-client"));
    }

    #[test]
    fn unknown_remote_failures_keep_a_bounded_diagnostic() {
        let message = describe(&remote(&"x".repeat(500)));
        assert!(message.starts_with("The upload failed. "));
        assert!(message.ends_with('…'));
        assert!(message.chars().count() < 280);
    }

    #[test]
    fn falls_back_to_the_cli_message() {
        let error = CliError::new(
            "entropy_unavailable",
            "Secure remote-name generation is unavailable.",
        );
        assert_eq!(
            describe(&error),
            "Secure remote-name generation is unavailable."
        );
    }
}
