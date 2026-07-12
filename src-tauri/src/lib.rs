mod epic;
mod i18n;
mod settings;
mod tray;

use tauri_plugin_updater::UpdaterExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
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
        update.install(bytes)?;
        app.restart();
    }
    Ok(())
}
