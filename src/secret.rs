//! The AI provider's API key. Kept in the system keychain (Secret Service on Linux, Keychain on macOS,
//! Credential Manager on Windows) and never in the settings database. Where there is no keychain (Android, or
//! a Linux session without a Secret Service) it falls back to a private file in the app's data directory, and
//! `Stored::File` says so.

use crate::models::AiProvider;

/// Where a key ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Keychain,
    File,
}

#[cfg(not(test))]
mod real {
    use super::*;

    const SERVICE: &str = "tanqi-music";

    fn account(provider: AiProvider) -> String {
        format!("ai-key-{}", provider.id())
    }

    fn file(provider: AiProvider) -> std::path::PathBuf {
        crate::app::app_data_dir().join(account(provider))
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn entry(provider: AiProvider) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account(provider)).ok()
    }

    pub fn get(provider: AiProvider) -> Option<String> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        if let Some(Ok(key)) = entry(provider).map(|e| e.get_password()) {
            return Some(key).filter(|k| !k.is_empty());
        }
        std::fs::read_to_string(file(provider)).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn set(provider: AiProvider, key: &str) -> Result<Stored, String> {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        if entry(provider).map(|e| e.set_password(key)).is_some_and(|r| r.is_ok()) {
            // A key left in the file from an earlier fallback is no longer wanted.
            let _ = std::fs::remove_file(file(provider));
            return Ok(Stored::Keychain);
        }
        let path = file(provider);
        std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("."))).map_err(|e| e.to_string())?;
        std::fs::write(&path, key).map_err(|e| format!("couldn't save the key: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Stored::File)
    }

    pub fn clear(provider: AiProvider) {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        if let Some(e) = entry(provider) {
            let _ = e.delete_credential();
        }
        let _ = std::fs::remove_file(file(provider));
    }
}

/// Tests never touch the real keychain.
#[cfg(test)]
mod real {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static KEYS: Mutex<Option<HashMap<&'static str, String>>> = Mutex::new(None);

    pub fn get(provider: AiProvider) -> Option<String> {
        KEYS.lock().unwrap().as_ref().and_then(|m| m.get(provider.id()).cloned())
    }
    pub fn set(provider: AiProvider, key: &str) -> Result<Stored, String> {
        KEYS.lock().unwrap().get_or_insert_with(HashMap::new).insert(provider.id(), key.to_string());
        Ok(Stored::Keychain)
    }
    pub fn clear(provider: AiProvider) {
        if let Some(m) = KEYS.lock().unwrap().as_mut() {
            m.remove(provider.id());
        }
    }
}

/// The saved key for `provider`, if any.
pub fn ai_key(provider: AiProvider) -> Option<String> {
    real::get(provider)
}

/// Saves `key` (trimmed) for `provider`; an empty key removes it.
pub fn set_ai_key(provider: AiProvider, key: &str) -> Result<Stored, String> {
    let key = key.trim();
    if key.is_empty() {
        real::clear(provider);
        return Ok(Stored::Keychain);
    }
    real::set(provider, key)
}

pub fn clear_ai_key(provider: AiProvider) {
    real::clear(provider)
}
