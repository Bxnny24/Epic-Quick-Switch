//! Saving the current Epic session and switching between saved accounts.
//!
//! The mechanism (no admin rights required):
//!   1. Kill the launcher FIRST — it rewrites `GameUserSettings.ini` on exit,
//!      so writing while it runs would be clobbered. Its helper processes are
//!      killed too: orphaned ones stall the next launcher start for minutes.
//!   2. Snapshot the outgoing session into the app's own store (every switch
//!      refreshes the token of the account being switched away from).
//!   3. Write the target account's `[RememberMe]` token into the ini and its
//!      `AccountId` into the registry.
//!   4. Relaunch the launcher minimized (`-silent`); it logs in with the
//!      restored token, no password prompt.
//!
//! Saving (`save_current`) does NOT kill the launcher: reads are safe while
//! it runs, and the on-disk token is current from the moment of login.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::epic::accounts::SessionState;
use crate::epic::{accounts, ini, logs, paths, registry, store};

/// How long to wait for the launcher to disappear after the force-kill. Epic
/// tears down several helper processes, so this is longer than Steam's.
const KILL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Grace period after the kill so file handles and the final ini flush settle.
const SETTLE_DELAY: Duration = Duration::from_millis(500);
/// `CREATE_NO_WINDOW`: stops console helpers (tasklist/taskkill) from flashing.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Everything Epic spawns. Order matters: the launcher first, so it cannot
/// respawn helpers; leftover helpers stall the next start by 1–2 minutes.
const EPIC_PROCESSES: [&str; 6] = [
    "EpicGamesLauncher.exe",
    "EpicWebHelper.exe",
    "UnrealCEFSubProcess.exe",
    "EOSOverlayRenderer-Win64-Shipping.exe",
    "EpicOnlineServicesUserHelper.exe",
    "CrashReportClient.exe",
];

/// Why saving the current session failed — mapped to localized text in the tray.
#[derive(Debug)]
pub enum SaveError {
    /// No launcher config on this machine (never run / not installed).
    NoLauncherData,
    /// Logged out, "Remember me" off, or the token is a logout placeholder.
    NoSession,
    /// No account ID in the registry and none recoverable from the logs.
    NoAccountId,
    /// The snapshot store could not be written.
    Store(String),
}

/// The account a successful `save_current` stored. The tray currently shows
/// no success dialog (the new entry appearing IS the feedback), so the fields
/// exist for future use (e.g. a toast).
#[allow(dead_code)]
pub struct SavedAccount {
    pub account_id: String,
    pub display_name: String,
}

/// Snapshot the launcher's current session into the store. Read-only towards
/// the launcher, so it works while the launcher is running.
pub fn save_current(app: &AppHandle) -> Result<SavedAccount, SaveError> {
    let location = paths::resolve_ini().map_err(|_| SaveError::NoLauncherData)?;
    let file = ini::load(&location.primary).map_err(|_| SaveError::NoLauncherData)?;
    let remember = ini::read_remember_me(&file).ok_or(SaveError::NoSession)?;
    if !remember.is_valid() {
        return Err(SaveError::NoSession);
    }
    let data = remember.data.expect("validated above");

    let account_id = registry::account_id()
        .or_else(|| logs::latest_identity().map(|i| i.account_id))
        .ok_or(SaveError::NoAccountId)?;

    let log_name = logs::username_for(&account_id);

    let mut store = store::AccountStore::load(app);
    store.upsert_session(&account_id, data, log_name);
    store.save().map_err(SaveError::Store)?;

    let display_name = store
        .get(&account_id)
        .map(|a| a.display_name.clone())
        .unwrap_or_default();
    Ok(SavedAccount { account_id, display_name })
}

/// Switch the live Epic session to the saved account `account_id`.
pub fn switch_account(app: &AppHandle, account_id: &str) -> Result<(), String> {
    let mut store = store::AccountStore::load(app);
    let target = store
        .get(account_id)
        .cloned()
        .ok_or_else(|| "Account not found in the switcher.".to_string())?;
    if target.remember_me_data.len() <= ini::MIN_DATA_LEN {
        return Err(
            "The saved session for this account is invalid or expired. Log in to the Epic Games Launcher and use 'Save current account' again."
                .to_string(),
        );
    }

    // Resolve everything BEFORE killing anything: never take the launcher
    // down if it cannot be relaunched or the config cannot be found.
    let exe = paths::launcher_exe()
        .ok_or_else(|| "Epic Games Launcher not found.".to_string())?;
    let location = paths::resolve_ini()?;

    if is_launcher_running() {
        kill_launcher()?;
        thread::sleep(SETTLE_DELAY);
    }

    // Snapshot the outgoing session (post-kill: the launcher has flushed its
    // final ini state). This keeps the away-account's token fresh — the best
    // defense against expiring snapshots.
    if let Ok(file) = ini::load(&location.primary) {
        if let Some(remember) = ini::read_remember_me(&file) {
            if remember.is_valid() {
                if let Some(current_id) = registry::account_id() {
                    if !current_id.eq_ignore_ascii_case(&target.account_id)
                        && store.get(&current_id).is_some()
                    {
                        let data = remember.data.expect("validated above");
                        store.upsert_session(&current_id, data, None);
                    }
                }
            }
        }
    }

    // Write the target session: primary is authoritative, the mirror (the
    // other candidate ini, if it exists) is best-effort so no launcher build
    // reads a stale token.
    write_session(&location.primary, &target.remember_me_data)?;
    if let Some(mirror) = &location.mirror {
        let _ = write_session(mirror, &target.remember_me_data);
    }

    // Registry identity: consistency polish, not load-bearing — the ini token
    // is what logs in. Warn-and-continue on failure.
    let _ = registry::set_account_id(&target.account_id);

    silent_command(&exe)
        .arg("-silent")
        .spawn()
        .map_err(|e| format!("failed to relaunch the Epic Games Launcher: {e}"))?;

    store.touch_last_used(&target.account_id);
    store.save()?;

    Ok(())
}

/// Whether the launcher rejected the restored token (used by the tray's
/// post-switch check ~20s after a switch): a logged-out live session means
/// the snapshot is stale.
pub fn session_rejected() -> bool {
    matches!(accounts::live_session(), SessionState::LoggedOut)
}

fn write_session(path: &std::path::Path, data: &str) -> Result<(), String> {
    let mut file = match ini::load(path) {
        Ok(file) => file,
        // A missing mirror/primary is created from scratch.
        Err(_) if !path.exists() => ini::IniFile {
            lines: Vec::new(),
            encoding: ini::IniEncoding::Utf8 { bom: false },
            newline: "\r\n",
            trailing_newline: false,
        },
        Err(e) => return Err(e),
    };
    ini::write_remember_me(&mut file, true, data);
    ini::save(path, &file)
}

/// Build a `Command` that never pops up a console window.
fn silent_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Whether the launcher's main process is currently running.
pub fn is_launcher_running() -> bool {
    silent_command("tasklist")
        .args(["/FI", "IMAGENAME eq EpicGamesLauncher.exe", "/NH", "/FO", "CSV"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("epicgameslauncher.exe")
        })
        .unwrap_or(false)
}

/// Force-kill the launcher and all of its helper processes, then confirm the
/// main process is gone. Epic ignores graceful close requests, and helpers
/// orphaned by a partial kill make the next start hang — so all six images
/// are force-killed every time.
fn kill_launcher() -> Result<(), String> {
    for image in EPIC_PROCESSES {
        let _ = silent_command("taskkill").args(["/F", "/IM", image]).output();
    }

    let start = Instant::now();
    while start.elapsed() < KILL_CONFIRM_TIMEOUT {
        if !is_launcher_running() {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err("The Epic Games Launcher did not shut down in time.".to_string())
}
