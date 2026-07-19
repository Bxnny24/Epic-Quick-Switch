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
//!   4. Relaunch the launcher in the foreground (no `-silent`); it logs in
//!      with the restored token, no password prompt.
//!
//! Saving (`save_current`) does NOT kill the launcher: reads are safe while
//! it runs, and the on-disk token is current from the moment of login.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use sysinfo::System;
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

/// Launcher-unique image names, safe to force-kill by name.
const LAUNCHER_PROCESSES: [&str; 2] = ["EpicGamesLauncher.exe", "EpicWebHelper.exe"];

/// Generic Unreal/EOS image names shared with the Unreal Editor and other UE
/// games. Killing these by name would take down unrelated apps, so only
/// instances living under the launcher's own tree are terminated (by PID).
const SCOPED_PROCESSES: [&str; 4] = [
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
    /// No account ID in the registry (e.g. the launcher is still starting up
    /// right after a switch — logs are deliberately NOT used as a fallback).
    NoAccountId,
    /// The snapshot store could not be written.
    Store(String),
}

/// Why a switch failed — mapped to localized dialog text in the tray. The
/// `String` payloads carry the technical reason, appended to the localized
/// message as a detail line.
#[derive(Debug)]
pub enum SwitchError {
    /// The clicked account is no longer in the store (stale menu entry).
    AccountMissing,
    /// The stored token is too short to be a real session.
    SnapshotInvalid,
    /// The snapshot was marked stale after Epic rejected it — switching would
    /// only log the user out, so it is refused until re-saved.
    SnapshotStale,
    LauncherNotFound,
    /// No launcher config found (never run / not installed).
    ConfigNotFound,
    /// Could not determine whether the launcher is running (fail closed).
    CheckFailed(String),
    KillTimeout,
    /// Persisting the outgoing session snapshot failed (post-kill).
    StoreSave(String),
    /// Writing the session ini failed (post-kill).
    IniWrite(String),
    /// Relaunching the launcher failed (post-kill).
    Relaunch(String),
}

/// What a successful `switch_account` actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// Full switch: launcher killed, token written, launcher relaunched. The
    /// caller should schedule the post-switch rejection check.
    Switched,
    /// The target already owned the live session and the launcher is running —
    /// nothing was touched. No rejection check must be scheduled (a manual
    /// logout right after the click would be misread as an expired token).
    AlreadyActive,
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
    let _guard = store::lock();
    let location = paths::resolve_ini().map_err(|_| SaveError::NoLauncherData)?;
    let file = ini::load(&location.primary).map_err(|_| SaveError::NoLauncherData)?;
    let remember = ini::read_remember_me(&file).ok_or(SaveError::NoSession)?;
    if !remember.is_valid() {
        return Err(SaveError::NoSession);
    }
    let data = remember.data.expect("validated above");

    // The account being saved is whoever owns the launcher's live session:
    // always the registry AccountId, never the logs. The launcher records
    // `-epicusername/-epicuserid` only on GAME LAUNCH, so right after a switch
    // or login the newest log identity still names the PREVIOUS account —
    // trusting it here would file this session's token under the wrong
    // account. If the registry has no id yet (launcher still starting up after
    // a switch), fail closed so the user just retries once it has settled.
    let account_id = registry::account_id().ok_or(SaveError::NoAccountId)?;

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

/// Silent best-effort capture of the current session for the tray's session
/// watcher: whoever is logged in gets saved/refreshed without any dialogs.
/// Returns `true` when the store changed (the caller refreshes the tray).
///
/// The caller must only invoke this on a QUIET watcher tick (no watch-key
/// change for a full interval): the registry AccountId survives a logout, so
/// sampling mid-login-sequence could file the new account's token under the
/// previous account's id. A quiet tick proves ini and registry were stable.
///
/// Note: the upsert bumps `saved_at`, which orders never-switched accounts in
/// the menu — benign (identical to a manual save), and the identical-token
/// skip below prevents gratuitous bumps.
pub fn auto_capture(app: &AppHandle) -> bool {
    // Same lock as switching/saving: a capture can never observe a switch's
    // intermediate ini/registry state.
    let _guard = store::lock();

    let Ok(location) = paths::resolve_ini() else {
        return false;
    };
    let Ok(file) = ini::load(&location.primary) else {
        return false;
    };
    // One ini read decides validity AND supplies the token (no re-read gap).
    // Logout placeholders and rejected sessions fail is_valid() here.
    let Some(remember) = ini::read_remember_me(&file) else {
        return false;
    };
    if !remember.is_valid() {
        return false;
    }
    let data = remember.data.expect("validated above");

    // Identity comes from the registry only — fail closed, like save_current.
    let Some(account_id) = registry::account_id() else {
        return false;
    };

    let mut store = store::AccountStore::load(app);
    // Explicitly removed accounts stay removed until a MANUAL save: the check
    // must precede upsert_session, which lifts tombstones.
    if store.is_removed(&account_id) {
        return false;
    }
    // Unchanged token: nothing to do. Must also precede the upsert (it would
    // bump saved_at and clear stale even for identical data), and keeps disk
    // writes rare despite the launcher's frequent ini touches.
    if store
        .get(&account_id)
        .is_some_and(|a| a.remember_me_data == data)
    {
        return false;
    }

    // Only now the expensive step (scanning launcher logs for the username).
    let log_name = logs::username_for(&account_id);
    store.upsert_session(&account_id, data, log_name);
    store.save().is_ok()
}

/// Switch the live Epic session to the saved account `account_id`.
pub fn switch_account(app: &AppHandle, account_id: &str) -> Result<SwitchOutcome, SwitchError> {
    // Serialize the whole switch: this prevents two interleaved kill/relaunch
    // sequences, stops a concurrent store write from clobbering our updates,
    // and (as the app's single "busy" mutex) blocks the auto-updater from
    // exiting the process mid-switch.
    let _guard = store::lock();

    let mut store = store::AccountStore::load(app);
    let target = store
        .get(account_id)
        .cloned()
        .ok_or(SwitchError::AccountMissing)?;
    if target.remember_me_data.len() <= ini::MIN_DATA_LEN {
        return Err(SwitchError::SnapshotInvalid);
    }
    // A token Epic already rejected can only log the user out — refuse the
    // switch until the account is re-saved (which clears `stale`).
    if target.stale {
        return Err(SwitchError::SnapshotStale);
    }

    // Already active AND the launcher is actually running? Then a "switch"
    // would only kill and relaunch the very session the user is on (possible
    // via a stale menu, or while the registry id was briefly unreadable and no
    // entry was disabled). Treat as success without touching anything. When
    // the launcher is NOT running (e.g. a previous switch wrote the token but
    // failed to relaunch), fall through: the full flow is what starts it.
    if let SessionState::LoggedIn { account_id: Some(ref live_id) } = accounts::live_session() {
        if live_id.eq_ignore_ascii_case(&target.account_id)
            && is_launcher_running().unwrap_or(false)
        {
            store.touch_last_used(&target.account_id);
            let _ = store.save();
            return Ok(SwitchOutcome::AlreadyActive);
        }
    }

    // Resolve everything BEFORE killing anything: never take the launcher
    // down if it cannot be relaunched or the config cannot be found.
    let exe = paths::launcher_exe().ok_or(SwitchError::LauncherNotFound)?;
    let location = paths::resolve_ini().map_err(|_| SwitchError::ConfigNotFound)?;

    // Fail closed: if we cannot tell whether the launcher is running, abort
    // rather than skip the kill and let a live launcher clobber our write.
    if is_launcher_running().map_err(SwitchError::CheckFailed)? {
        kill_launcher(&exe).map_err(|_| SwitchError::KillTimeout)?;
        thread::sleep(SETTLE_DELAY);
    } else {
        // Main process already gone (crash/manual kill) — still sweep up any
        // orphaned processes, which would otherwise stall the relaunch. This
        // includes the launcher-unique images (EpicWebHelper.exe!), not just
        // the scoped Unreal/EOS helpers.
        kill_launcher_images();
        kill_scoped_processes(&exe);
    }

    // Snapshot the outgoing session (post-kill: the launcher has flushed its
    // final ini state) and PERSIST it before the destructive write below, so a
    // later spawn/save failure cannot lose the only fresh copy of that token.
    // Never-saved accounts are added too — silently discarding a live session
    // (forcing a manual re-login later) is the worse failure.
    let mut outgoing_saved = false;
    if let Ok(file) = ini::load(&location.primary) {
        if let Some(remember) = ini::read_remember_me(&file) {
            if remember.is_valid() {
                if let Some(current_id) = registry::account_id() {
                    // Explicitly removed accounts are NOT re-captured: the
                    // user deleted that token on purpose (tombstone lifts on
                    // a manual re-save).
                    if !current_id.eq_ignore_ascii_case(&target.account_id)
                        && !store.is_removed(&current_id)
                    {
                        let data = remember.data.expect("validated above");
                        let log_name = logs::username_for(&current_id);
                        store.upsert_session(&current_id, data, log_name);
                        outgoing_saved = true;
                    }
                }
            }
        }
    }
    if outgoing_saved {
        store.save().map_err(SwitchError::StoreSave)?;
    }

    // Write the target session: primary is authoritative, the mirror (the
    // other candidate ini, if it exists) is best-effort so no launcher build
    // reads a stale token.
    write_session(&location.primary, &target.remember_me_data).map_err(SwitchError::IniWrite)?;
    if let Some(mirror) = &location.mirror {
        let _ = write_session(mirror, &target.remember_me_data);
    }

    // Registry identity: consistency polish, not load-bearing — the ini token
    // is what logs in. Best-effort: a failure here is invisible by design,
    // because failing the whole switch over it would be worse.
    let _ = registry::set_account_id(&target.account_id);

    // Relaunch in the FOREGROUND so the launcher visibly comes up on the
    // switched account. Dropping `-silent` (which starts it minimized to the
    // tray) is what makes the window open; CREATE_NO_WINDOW from
    // `silent_command` only suppresses a console window and is ignored for the
    // launcher's GUI process, so it does not hide the window.
    silent_command(&exe)
        .spawn()
        .map_err(|e| SwitchError::Relaunch(e.to_string()))?;

    // Capture the display name at switch time from the target's OWN log
    // identity (looked up by account ID, so never the outgoing account's
    // name). Best-effort and non-custom-only: this makes the switch itself
    // refresh the name, so a "save current account" done right afterwards no
    // longer depends on logs that lag the just-restored session.
    store.refresh_log_name(&target.account_id, logs::username_for(&target.account_id));

    // Best-effort bookkeeping: the switch has already succeeded, so a failure
    // to persist `last_used` must not report the whole switch as failed.
    store.touch_last_used(&target.account_id);
    let _ = store.save();

    Ok(SwitchOutcome::Switched)
}

/// Whether the launcher rejected the token just restored for `expected_id`
/// (used by the tray's post-switch check ~20s after a switch). A logged-out
/// live session only counts when the registry identity still names the account
/// we switched to — otherwise a newer switch or a manual logout is being
/// misattributed to this one.
pub fn session_rejected(expected_id: &str) -> bool {
    matches!(accounts::live_session(), SessionState::LoggedOut)
        && registry::account_id().is_some_and(|id| id.eq_ignore_ascii_case(expected_id))
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

/// Whether the launcher's main process is currently running. Returns `Err`
/// when the check itself could not run (e.g. `tasklist` blocked by policy),
/// so callers can fail closed instead of assuming "not running".
pub fn is_launcher_running() -> Result<bool, String> {
    let output = silent_command("tasklist")
        .args(["/FI", "IMAGENAME eq EpicGamesLauncher.exe", "/NH", "/FO", "CSV"])
        .output()
        .map_err(|e| format!("could not check whether the Epic Games Launcher is running: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .to_lowercase()
        .contains("epicgameslauncher.exe"))
}

/// Force-kill the launcher and its helper processes, then confirm the main
/// process is gone. Epic ignores graceful close requests, and helpers orphaned
/// by a partial kill make the next start hang — so the launcher-owned images
/// are killed by name and the generic Unreal/EOS helpers are killed by PID
/// only within the launcher's own install tree (`kill_scoped_processes`).
fn kill_launcher(exe: &Path) -> Result<(), String> {
    kill_launcher_images();
    kill_scoped_processes(exe);

    let start = Instant::now();
    while start.elapsed() < KILL_CONFIRM_TIMEOUT {
        // Treat a check failure as "still running" so a broken tasklist falls
        // through to the timeout error rather than falsely confirming the kill.
        if !is_launcher_running().unwrap_or(true) {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err("The Epic Games Launcher did not shut down in time.".to_string())
}

/// Force-kill the launcher-unique images by name (main process + web helper).
fn kill_launcher_images() {
    for image in LAUNCHER_PROCESSES {
        let _ = silent_command("taskkill").args(["/F", "/IM", image]).output();
    }
}

/// Kill the generic Unreal/EOS helper processes, but only instances whose
/// executable lives under the launcher's own tree (or the Epic Online Services
/// runtime) — leaving the Unreal Editor and third-party UE games untouched.
fn kill_scoped_processes(launcher_exe: &Path) {
    // The launcher's install root, e.g. `...\Epic Games\Launcher`.
    let launcher_root = launcher_exe
        .ancestors()
        .find(|a| {
            a.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Launcher"))
        })
        .map(|p| p.to_string_lossy().to_lowercase());

    let sys = System::new_all();
    for process in sys.processes().values() {
        let name = process.name().to_string_lossy();
        if !SCOPED_PROCESSES.iter().any(|n| name.eq_ignore_ascii_case(n)) {
            continue;
        }
        let Some(exe) = process.exe() else {
            continue;
        };
        let path = exe.to_string_lossy().to_lowercase();
        let under_launcher = launcher_root.as_deref().is_some_and(|r| path.starts_with(r));
        let under_eos = path.contains("\\epic online services\\");
        if under_launcher || under_eos {
            let _ = process.kill();
        }
    }
}
