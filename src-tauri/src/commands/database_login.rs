//! Optional "remember this MariaDB admin login" storage, one file per workspace.
//!
//! The whole login is encrypted with Windows DPAPI (current user), the same
//! protection used for saved RCON passwords, and is only written when the user
//! turns on "Remember on this PC".

use std::{fs, path::PathBuf};

use tauri::{AppHandle, Manager};

use crate::models::mariadb::MariaDBCredentials;

use super::fxserver::{decrypt_secret, encrypt_secret};

fn login_path(app: &AppHandle, workspace_id: &str) -> Result<PathBuf, String> {
    if !valid_workspace_id(workspace_id) {
        return Err("The workspace ID is invalid.".to_string());
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve application data directory: {error}"))?
        .join("mariadb-login")
        .join(format!("{workspace_id}.dat")))
}

fn valid_workspace_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

#[tauri::command]
pub async fn save_mariadb_login(
    app: AppHandle,
    workspace_id: String,
    credentials: MariaDBCredentials,
) -> Result<(), String> {
    let path = login_path(&app, &workspace_id)?;
    super::run_blocking(move || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create the saved login folder: {error}"))?;
        }
        let json = serde_json::to_vec(&credentials)
            .map_err(|error| format!("Failed to encode the login: {error}"))?;
        let encrypted = encrypt_secret(&json)?;
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, encode_hex(&encrypted))
            .map_err(|error| format!("Failed to save the login securely: {error}"))?;
        fs::rename(&temporary, &path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("Failed to save the login securely: {error}")
        })
    })
    .await
}

/// Returns `None` when nothing is saved or it cannot be read (for example it
/// was saved by a different Windows user); the UI then just asks again.
#[tauri::command]
pub async fn load_mariadb_login(
    app: AppHandle,
    workspace_id: String,
) -> Result<Option<MariaDBCredentials>, String> {
    let path = login_path(&app, &workspace_id)?;
    super::run_blocking(move || {
        let Ok(text) = fs::read_to_string(path) else {
            return Ok(None);
        };
        let credentials = decode_hex(text.trim())
            .and_then(|bytes| decrypt_secret(&bytes).ok())
            .and_then(|json| serde_json::from_slice::<MariaDBCredentials>(&json).ok());
        Ok(credentials)
    })
    .await
}

#[tauri::command]
pub async fn clear_mariadb_login(app: AppHandle, workspace_id: String) -> Result<(), String> {
    let path = login_path(&app, &workspace_id)?;
    super::run_blocking(move || match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove the saved login: {error}")),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_garbage() {
        let bytes = vec![0u8, 1, 127, 128, 255];
        assert_eq!(decode_hex(&encode_hex(&bytes)), Some(bytes));
        assert_eq!(decode_hex("abc"), None);
        assert_eq!(decode_hex("zz"), None);
        assert_eq!(decode_hex("é"), None);
    }

    #[test]
    fn workspace_ids_are_restricted() {
        assert!(valid_workspace_id("default"));
        assert!(valid_workspace_id("a1_b-2"));
        assert!(!valid_workspace_id(""));
        assert!(!valid_workspace_id("../evil"));
        assert!(!valid_workspace_id("a/b"));
        assert!(!valid_workspace_id(&"a".repeat(65)));
    }
}
