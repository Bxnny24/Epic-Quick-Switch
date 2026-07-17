mod epic;
mod i18n;
mod settings;
mod tray;

use tauri_plugin_updater::UpdaterExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch must not be a silent no-op ("did it even
            // start?"). The dialog runs on a worker thread so the modal does
            // not block this instance's event loop.
            let message = i18n::labels(&settings::language(app)).already_running;
            std::thread::spawn(move || tray::show_info("Epic Quick Switch", message));
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            settings::ensure_autostart_default(app.handle());
            tray::setup(app.handle())?;
            // Background auto-update check.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = try_update(handle).await;
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // The app is tray-only: keep it running even with no windows. Only
            // an explicit exit (the Quit menu calling app.exit) stops it.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

async fn try_update(app: tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    if let Some(update) = app.updater()?.check().await? {
        // Download first, then gate the install (which terminates the process
        // on Windows) behind the shared busy lock, so an update can never tear
        // the process down in the middle of an in-flight account switch.
        let bytes = update.download(|_, _| {}, || {}).await?;
        let _guard = epic::store::lock();
        // A quit may have been racing us for the lock: installing now would
        // turn the user's quit into an install-and-restart. Let the exit win;
        // the update applies on the next start's check instead.
        if tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        update.install(bytes)?;
        // Quit may have been clicked during the (multi-second) install: honor
        // it — exit instead of restarting into a session the user just closed.
        if tray::QUITTING.load(std::sync::atomic::Ordering::SeqCst) {
            app.exit(0);
            return Ok(());
        }
        app.restart();
    }
    Ok(())
}
