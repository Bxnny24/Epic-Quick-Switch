//! The account model exposed to the tray, and live-session probing.

use serde::Serialize;
use tauri::AppHandle;

use crate::epic::{ini, paths, registry, store};

/// An account as presented to the UI. Field names are camelCase in JSON.
/// Deliberately does NOT carry the session token, so it can never leak into
/// tooltips, logs or the Tauri bridge.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub account_id: String,
    pub display_name: String,
    pub saved_at: u64,
    pub last_used: Option<u64>,
    /// True if this account owns the launcher's current live session.
    pub is_current: bool,
    /// True if the last switch revealed an expired/rejected token.
    pub stale: bool,
}

/// The machine's live Epic session, read-only (safe with the launcher open).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// A valid `[RememberMe]` token exists. `account_id` comes from the
    /// registry and can be `None` if the key is missing.
    LoggedIn { account_id: Option<String> },
    /// The ini exists but holds no usable session (logged out).
    LoggedOut,
    /// No launcher config found at all (never run / not installed).
    NoLauncherData,
}

pub fn live_session() -> SessionState {
    let Ok(location) = paths::resolve_ini() else {
        return SessionState::NoLauncherData;
    };
    let Ok(file) = ini::load(&location.primary) else {
        return SessionState::NoLauncherData;
    };
    match ini::read_remember_me(&file) {
        Some(remember) if remember.is_valid() => {
            SessionState::LoggedIn { account_id: registry::account_id() }
        }
        _ => SessionState::LoggedOut,
    }
}

/// All saved accounts — the active one first, then most-recently-used. The
/// registry `AccountId` alone is not trusted for "current": it survives a
/// logout, so it only counts while the session ini says "logged in".
pub fn list_accounts(app: &AppHandle) -> Result<Vec<Account>, String> {
    let store = store::AccountStore::load(app);

    let current_id = match live_session() {
        SessionState::LoggedIn { account_id: Some(id) } => Some(id.to_lowercase()),
        _ => None,
    };

    let mut accounts: Vec<Account> = store
        .accounts()
        .iter()
        .map(|a| Account {
            account_id: a.account_id.clone(),
            display_name: a.display_name.clone(),
            saved_at: a.saved_at,
            last_used: a.last_used,
            is_current: current_id.as_deref() == Some(a.account_id.to_lowercase().as_str()),
            stale: a.stale,
        })
        .collect();

    accounts.sort_by_key(|a| {
        (
            std::cmp::Reverse(a.is_current),
            std::cmp::Reverse(a.last_used.unwrap_or(a.saved_at)),
        )
    });

    Ok(accounts)
}

/// Cheap composite key for the tray watcher: the registry account ID plus the
/// session ini's modification time and size. Any external switch, login or
/// logout changes it without hashing the multi-KB token every tick.
pub fn watch_key() -> String {
    let id = registry::account_id().unwrap_or_default().to_lowercase();
    let ini_stamp = paths::resolve_ini()
        .ok()
        .and_then(|loc| std::fs::metadata(&loc.primary).ok())
        .map(|m| format!("{:?}|{}", m.modified().ok(), m.len()))
        .unwrap_or_default();
    format!("{id}|{ini_stamp}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test against the real machine. Ignored by default (needs Epic).
    /// Run with: cargo test -- --ignored --nocapture print_live_session
    #[test]
    #[ignore]
    fn print_live_session() {
        println!("live session: {:?}", live_session());
        println!("watch key: {}", watch_key());
    }
}
