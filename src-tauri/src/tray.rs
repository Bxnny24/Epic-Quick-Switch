//! System tray — the entire UI. A native menu lists the saved Epic accounts
//! (with generated initials badges), a "save current account" action, account
//! removal, and settings; the tray icon shows the active account's badge.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    image::Image,
    menu::{CheckMenuItem, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, SubmenuBuilder},
    tray::TrayIconBuilder,
    AppHandle, Wry,
};
use tauri_plugin_autostart::ManagerExt;

use crate::epic::{self, icon, store, switch, Account};
use crate::{i18n, settings};

pub const TRAY_ID: &str = "main-tray";
const TRAY_ICON_SIZE: u32 = 32;
const MENU_ICON_SIZE: u32 = 18;
/// How often to poll for session changes made outside this app.
const WATCH_INTERVAL: Duration = Duration::from_secs(3);
/// How long after a switch to check whether Epic accepted the restored token.
const STALE_CHECK_DELAY: Duration = Duration::from_secs(20);

/// Bumped on every switch. A pending post-switch stale check bails out if a
/// newer switch has started, so it can never blame a superseded account.
static SWITCH_GENERATION: AtomicU64 = AtomicU64::new(0);

/// True while a switch is in flight. Further switch clicks are ignored until
/// it completes — a second click would otherwise queue a full second
/// kill/relaunch cycle behind the store lock (launcher dies twice in a row).
static SWITCH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Set when the user clicked Quit. The auto-updater checks it after acquiring
/// the busy lock so a quit cannot morph into an update-install-and-restart.
pub(crate) static QUITTING: AtomicBool = AtomicBool::new(false);

/// Releases [`SWITCH_IN_PROGRESS`] on drop, so the flag can never leak (and
/// permanently block switching) if the switch thread panics.
struct SwitchInProgressGuard;

impl Drop for SwitchInProgressGuard {
    fn drop(&mut self) {
        SWITCH_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// Localized dialog body for a failed switch; technical detail appended.
fn switch_error_message(l: &i18n::Labels, error: &switch::SwitchError) -> String {
    use switch::SwitchError::*;
    match error {
        AccountMissing => l.err_account_missing.to_string(),
        SnapshotInvalid => l.err_snapshot_invalid.to_string(),
        SnapshotStale => l.err_snapshot_stale.to_string(),
        LauncherNotFound => l.err_launcher_not_found.to_string(),
        ConfigNotFound => l.err_no_launcher_data.to_string(),
        CheckFailed(detail) => format!("{}\n\n{detail}", l.err_check_failed),
        KillTimeout => l.err_kill_timeout.to_string(),
        StoreSave(detail) => format!("{}\n\n{detail}", l.err_store_write),
        IniWrite(detail) => format!("{}\n\n{detail}", l.err_ini_write),
        Relaunch(detail) => format!("{}\n\n{detail}", l.err_relaunch),
    }
}

/// Display name: Epic username or short account ID, per the user's setting.
fn display_name(app: &AppHandle, account: &Account) -> String {
    if settings::name_mode(app) == "id" {
        let short: String = account.account_id.chars().take(8).collect();
        if !short.is_empty() {
            return short;
        }
    }
    account.display_name.clone()
}

/// Badge icon for a menu entry. The initial is derived from the same string
/// the label shows, so "Account ID" mode (privacy) never leaks the first
/// letter of the Epic username through the badge.
fn menu_icon(app: &AppHandle, account: &Account) -> Image<'static> {
    let label = display_name(app, account);
    let initial = icon::initial_for(&label, &account.account_id);
    let (rgba, size) = icon::badge_rgba(&account.account_id, initial, MENU_ICON_SIZE);
    Image::new_owned(rgba, size, size)
}

/// Build the full tray menu from the saved accounts and settings.
fn build_menu(app: &AppHandle, accounts: &[Account]) -> tauri::Result<Menu<Wry>> {
    let lang = settings::language(app);
    let mode = settings::name_mode(app);
    let l = i18n::labels(&lang);

    let menu = Menu::new(app)?;

    if accounts.is_empty() {
        let item = MenuItem::with_id(app, "noop", l.no_accounts, false, None::<&str>)?;
        menu.append(&item)?;
        let hint = MenuItem::with_id(app, "noop-hint", l.no_accounts_hint, false, None::<&str>)?;
        menu.append(&hint)?;
    } else {
        for account in accounts {
            let mut label = display_name(app, account);
            if account.is_current {
                label = format!("{label}  •  {}", l.active);
            }
            if account.stale {
                label = format!("{label}  ({})", l.expired);
            }
            let item = IconMenuItem::with_id(
                app,
                format!("switch:{}", account.account_id),
                label.as_str(),
                !account.is_current,
                Some(menu_icon(app, account)),
                None::<&str>,
            )?;
            menu.append(&item)?;
        }
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    let save = MenuItem::with_id(app, "save-current", l.save_current, true, None::<&str>)?;
    menu.append(&save)?;

    if !accounts.is_empty() {
        let mut remove_menu = SubmenuBuilder::new(app, l.remove_account);
        for account in accounts {
            let item = MenuItem::with_id(
                app,
                format!("remove:{}", account.account_id),
                display_name(app, account),
                true,
                None::<&str>,
            )?;
            remove_menu = remove_menu.item(&item);
        }
        menu.append(&remove_menu.build()?)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // Settings submenu: language, display name, autostart.
    let lang_en =
        CheckMenuItem::with_id(app, "lang:en", "English", true, lang == "en", None::<&str>)?;
    let lang_de =
        CheckMenuItem::with_id(app, "lang:de", "Deutsch", true, lang == "de", None::<&str>)?;
    let lang_menu = SubmenuBuilder::new(app, l.language)
        .item(&lang_en)
        .item(&lang_de)
        .build()?;

    let name_display = CheckMenuItem::with_id(
        app,
        "name:display",
        l.name_display,
        true,
        mode == "display",
        None::<&str>,
    )?;
    let name_id =
        CheckMenuItem::with_id(app, "name:id", l.name_id, true, mode == "id", None::<&str>)?;
    let name_menu = SubmenuBuilder::new(app, l.display_name)
        .item(&name_display)
        .item(&name_id)
        .build()?;

    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart =
        CheckMenuItem::with_id(app, "autostart", l.autostart, true, autostart_on, None::<&str>)?;

    let settings_menu = SubmenuBuilder::new(app, l.settings)
        .item(&lang_menu)
        .item(&name_menu)
        .item(&autostart)
        .build()?;
    menu.append(&settings_menu)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    menu.append(&quit)?;

    Ok(menu)
}

/// Rebuild and apply the tray menu and icon. Safe to call repeatedly.
pub fn refresh(app: &AppHandle) {
    let accounts = epic::list_accounts(app).unwrap_or_default();
    if let Ok(menu) = build_menu(app, &accounts) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    refresh_icon(app, &accounts);
}

fn refresh_icon(app: &AppHandle, accounts: &[Account]) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    // Only a genuinely ACTIVE account gets its badge on the tray. When nobody
    // is logged in, the default app icon is shown instead — a last-used badge
    // would be indistinguishable from "logged in as this account".
    match accounts.iter().find(|a| a.is_current) {
        Some(account) => {
            let label = display_name(app, account);
            let initial = icon::initial_for(&label, &account.account_id);
            let _ = tray.set_tooltip(Some(label));
            let (rgba, size) = icon::badge_rgba(&account.account_id, initial, TRAY_ICON_SIZE);
            let _ = tray.set_icon(Some(Image::new_owned(rgba, size, size)));
        }
        None => {
            let _ = tray.set_tooltip(Some("Epic Quick Switch"));
            if let Some(default) = app.default_window_icon() {
                let _ = tray.set_icon(Some(default.clone()));
            }
        }
    }
}

/// Create the tray icon and menu on startup.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let accounts = epic::list_accounts(app).unwrap_or_default();
    let menu = build_menu(app, &accounts)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Epic Quick Switch")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)?;
    refresh_icon(app, &accounts);
    start_session_watcher(app);
    Ok(())
}

/// Watch for session changes made outside this app (logins, logouts or
/// switches in the launcher itself) and refresh the tray on change.
fn start_session_watcher(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last = epic::accounts::watch_key();
        loop {
            std::thread::sleep(WATCH_INTERVAL);
            let now = epic::accounts::watch_key();
            if now != last {
                last = now;
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || refresh(&handle));
            }
        }
    });
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    if let Some(account_id) = id.strip_prefix("switch:") {
        switch_to(app, account_id.to_string());
    } else if id == "save-current" {
        save_current_clicked(app);
    } else if let Some(account_id) = id.strip_prefix("remove:") {
        remove_clicked(app, account_id.to_string());
    } else if id == "lang:en" {
        settings::set_language(app, "en");
        refresh(app);
    } else if id == "lang:de" {
        settings::set_language(app, "de");
        refresh(app);
    } else if id == "name:display" {
        settings::set_name_mode(app, "display");
        refresh(app);
    } else if id == "name:id" {
        settings::set_name_mode(app, "id");
        refresh(app);
    } else if id == "autostart" {
        let manager = app.autolaunch();
        let result = if manager.is_enabled().unwrap_or(false) {
            manager.disable()
        } else {
            manager.enable()
        };
        // A silent failure would just look like a checkmark refusing to move.
        if result.is_err() {
            let l = i18n::labels(&settings::language(app));
            show_error(l.autostart, l.autostart_failed);
        }
        // The user is now managing autostart explicitly — the first-run
        // default must never override this choice on a later start.
        settings::mark_autostart_configured(app);
        refresh(app);
    } else if id == "quit" {
        // Exit only once no switch/store operation is in flight: quitting
        // between the launcher kill and the relaunch would leave the launcher
        // dead and the outgoing token unsaved. The lock is taken on a worker
        // thread so a pending switch does not freeze the tray while quitting.
        // QUITTING additionally stops the auto-updater from turning this quit
        // into an update-install-and-restart if it wins the lock race.
        QUITTING.store(true, Ordering::SeqCst);
        let app = app.clone();
        std::thread::spawn(move || {
            let _guard = store::lock();
            app.exit(0);
        });
    }
}

/// Perform an account switch off the main thread, then refresh the tray and
/// verify (after a delay) that Epic accepted the restored session.
fn switch_to(app: &AppHandle, account_id: String) {
    let accounts = epic::list_accounts(app).unwrap_or_default();
    let Some(account) = accounts.into_iter().find(|a| a.account_id == account_id) else {
        // Stale menu entry (account was removed meanwhile): never a silent
        // no-op — say so and rebuild the menu.
        let l = i18n::labels(&settings::language(app));
        show_error(l.switch_failed, l.err_account_missing);
        refresh(app);
        return;
    };
    // One switch at a time: a second full kill/relaunch cycle would otherwise
    // queue up behind the store lock. Not silent — the user gets told.
    if SWITCH_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let l = i18n::labels(&settings::language(app));
        show_info("Epic Quick Switch", l.switch_busy);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let l = i18n::labels(&settings::language(&app));
        // Never fail silently: switching is the app's primary action, so
        // surface any error in a native dialog instead of leaving the user
        // guessing.
        // Guard released as soon as the switch itself finishes (also on
        // panic) — it must NOT stay held through the 20s stale check below.
        let guard = SwitchInProgressGuard;
        let result = switch::switch_account(&app, &account.account_id);
        // Only a REAL switch claims a generation: preflight failures and
        // already-active no-ops must not cancel a previous switch's pending
        // check. Claimed BEFORE the guard drops, so two back-to-back switches
        // can never claim generations out of order.
        let generation = match &result {
            Ok(switch::SwitchOutcome::Switched) => {
                Some(SWITCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1)
            }
            _ => None,
        };
        drop(guard);
        if let Err(error) = result {
            show_error(l.switch_failed, &switch_error_message(&l, &error));
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || refresh(&handle));
            return;
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || refresh(&handle));

        // Only a REAL switch schedules the rejection check — probing an
        // untouched session after a no-op would misread a manual logout as an
        // expired token.
        let Some(generation) = generation else {
            return;
        };

        // The token blob is opaque, so an expired session can only be seen
        // after the launcher tried it: if it logs the user out again, mark
        // the snapshot and tell the user how to fix it. Bail out if a newer
        // switch superseded this one (else we'd blame the wrong account).
        std::thread::sleep(STALE_CHECK_DELAY);
        if SWITCH_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        if switch::session_rejected(&account.account_id) {
            {
                let _guard = store::lock();
                let mut store = store::AccountStore::load(&app);
                store.mark_stale(&account.account_id);
                let _ = store.save();
            }
            let name = display_name(&app, &account);
            show_error(
                l.session_expired_title,
                &i18n::with_name(l.session_expired, &name),
            );
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || refresh(&handle));
        }
    });
}

/// Snapshot the current launcher session off the main thread.
fn save_current_clicked(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let l = i18n::labels(&settings::language(&app));
        match switch::save_current(&app) {
            Ok(_) => {}
            Err(error) => {
                let message = match error {
                    switch::SaveError::NoLauncherData => l.err_no_launcher_data.to_string(),
                    switch::SaveError::NoSession => l.err_no_session.to_string(),
                    switch::SaveError::NoAccountId => l.err_no_account_id.to_string(),
                    // Localized headline, technical reason as detail line.
                    switch::SaveError::Store(detail) => {
                        format!("{}\n\n{detail}", l.err_store_write)
                    }
                };
                show_error(l.save_failed, &message);
            }
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || refresh(&handle));
    });
}

/// Remove a saved account after a native confirmation. Only the snapshot in
/// this app is deleted — the Epic account and any live session are untouched.
fn remove_clicked(app: &AppHandle, account_id: String) {
    let accounts = epic::list_accounts(app).unwrap_or_default();
    let Some(account) = accounts.into_iter().find(|a| a.account_id == account_id) else {
        // Stale menu entry (already removed): give feedback and heal the menu
        // instead of silently doing nothing.
        let l = i18n::labels(&settings::language(app));
        show_error(l.remove_account, l.err_account_missing);
        refresh(app);
        return;
    };
    let name = display_name(app, &account);
    let app = app.clone();
    std::thread::spawn(move || {
        let l = i18n::labels(&settings::language(&app));
        if confirm(l.remove_account, &i18n::with_name(l.remove_confirm, &name)) {
            let _guard = store::lock();
            let mut store = store::AccountStore::load(&app);
            store.remove(&account.account_id);
            let _ = store.save();
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || refresh(&handle));
    });
}

// ---------------------------------------------------------------------------
// Native dialogs (user32 MessageBoxW — no extra dependency). Windows-only,
// matching the rest of the app.
// ---------------------------------------------------------------------------

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut core::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        u_type: u32,
    ) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

const MB_OK: u32 = 0x0000_0000;
const MB_YESNO: u32 = 0x0000_0004;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_ICONQUESTION: u32 = 0x0000_0020;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
/// Second button (No) is the default — a stray Enter must not confirm a
/// destructive action.
const MB_DEFBUTTON2: u32 = 0x0000_0100;
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const IDYES: i32 = 6;

/// Show a native modal error dialog so failures are never silent.
fn show_error(title: &str, message: &str) {
    let text = wide(message);
    let caption = wide(title);
    // SAFETY: `text` and `caption` are valid NUL-terminated UTF-16 buffers that
    // live until the call returns; a null owner shows an ownerless modal, which
    // is what a tray-only app needs.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

/// Native informational dialog (e.g. "already running" on a second launch).
pub(crate) fn show_info(title: &str, message: &str) {
    let text = wide(message);
    let caption = wide(title);
    // SAFETY: see show_error.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
}

/// Native yes/no confirmation; `true` when the user picked Yes. `No` is the
/// default button: these confirms guard destructive actions.
fn confirm(title: &str, message: &str) -> bool {
    let text = wide(message);
    let caption = wide(title);
    // SAFETY: see show_error.
    let choice = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND,
        )
    };
    choice == IDYES
}
