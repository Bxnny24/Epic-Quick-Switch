//! Persistent settings, stored in `settings.json` (via the store plugin) in
//! the app data directory. Read and written from Rust so the app needs no
//! window.

use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_store::StoreExt;

const STORE: &str = "settings.json";

/// The UI language: "en" or "de". Falls back to the OS locale.
pub fn language(app: &AppHandle) -> String {
    if let Ok(store) = app.store(STORE) {
        if let Some(value) = store.get("language") {
            if let Some(s) = value.as_str() {
                if s == "de" || s == "en" {
                    return s.to_string();
                }
            }
        }
    }
    detect_language()
}

pub fn set_language(app: &AppHandle, lang: &str) {
    if let Ok(store) = app.store(STORE) {
        store.set("language", json!(lang));
        let _ = store.save();
    }
}

/// How account names are shown: "display" (Epic username) or "id" (account ID).
pub fn name_mode(app: &AppHandle) -> String {
    if let Ok(store) = app.store(STORE) {
        if let Some(value) = store.get("nameMode") {
            if let Some(s) = value.as_str() {
                if s == "display" || s == "id" {
                    return s.to_string();
                }
            }
        }
    }
    "display".to_string()
}

pub fn set_name_mode(app: &AppHandle, mode: &str) {
    if let Ok(store) = app.store(STORE) {
        store.set("nameMode", json!(mode));
        let _ = store.save();
    }
}

/// On first run, enable "start with Windows" by default (once). If the user
/// later turns it off, it is not re-enabled. The configured flag is only set
/// once enabling actually succeeded — otherwise the default would silently
/// never apply (and never be retried on later starts).
pub fn ensure_autostart_default(app: &AppHandle) {
    if let Ok(store) = app.store(STORE) {
        if store.get("autostartConfigured").is_none() && app.autolaunch().enable().is_ok() {
            store.set("autostartConfigured", json!(true));
            let _ = store.save();
        }
    }
}

/// Record that autostart has been explicitly configured (first-run default or
/// a manual toggle). Once set, `ensure_autostart_default` never interferes
/// again — the user's choice must not be overridden by a later retry.
pub fn mark_autostart_configured(app: &AppHandle) {
    if let Ok(store) = app.store(STORE) {
        store.set("autostartConfigured", json!(true));
        let _ = store.save();
    }
}

fn detect_language() -> String {
    let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if locale.starts_with("de") {
        "de".to_string()
    } else {
        "en".to_string()
    }
}
