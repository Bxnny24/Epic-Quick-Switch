//! Reading and writing Epic-related values in the Windows registry.
//!
//! All writes are user-scoped (`HKCU`) and need no admin rights. `HKCR`/`HKLM`
//! are only consulted read-only to locate the launcher executable.

use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

/// Where the launcher keeps the active account's ID.
const IDENTIFIERS_KEY: &str = r"Software\Epic Games\Unreal Engine\Identifiers";
/// The launcher's URL-protocol handler; its command points at the exe.
const PROTOCOL_COMMAND_KEY: &str = r"com.epicgames.launcher\shell\open\command";

/// The account ID of the currently active Epic session.
///
/// This is the cheap, always-available identity key — the analog of Steam's
/// `AutoLoginUser`. Note it survives a logout, so it only names an account
/// while the session ini says "logged in".
pub fn account_id() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let identifiers = hkcu.open_subkey(IDENTIFIERS_KEY).ok()?;
    let id: String = identifiers.get_value("AccountId").ok()?;
    let id = id.trim().to_string();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// Point the launcher's identity at `account_id`. Writes to `HKCU` only, so
/// no admin rights are required.
pub fn set_account_id(account_id: &str) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (identifiers, _) = hkcu
        .create_subkey(IDENTIFIERS_KEY)
        .map_err(|e| format!("failed to open Epic identifiers registry key: {e}"))?;
    identifiers
        .set_value("AccountId", &account_id.to_string())
        .map_err(|e| format!("failed to set AccountId: {e}"))
}

/// The default value of the `com.epicgames.launcher` protocol handler, e.g.
/// `"C:\...\EpicGamesLauncher.exe" "%1"`. Checked in `HKCR`, then
/// `HKLM\SOFTWARE\Classes`.
pub fn launcher_open_command() -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    if let Ok(key) = hkcr.open_subkey(PROTOCOL_COMMAND_KEY) {
        if let Ok(command) = key.get_value::<String, _>("") {
            if !command.trim().is_empty() {
                return Some(command);
            }
        }
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey(format!(r"SOFTWARE\Classes\{PROTOCOL_COMMAND_KEY}")) {
        if let Ok(command) = key.get_value::<String, _>("") {
            if !command.trim().is_empty() {
                return Some(command);
            }
        }
    }

    None
}
