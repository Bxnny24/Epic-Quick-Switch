//! Minimal tray-menu translations (English / German).
//!
//! Labels containing `{name}` are templates; render them with [`with_name`].

pub struct Labels {
    pub settings: &'static str,
    pub language: &'static str,
    pub autostart: &'static str,
    pub display_name: &'static str,
    pub name_display: &'static str,
    pub name_id: &'static str,
    pub quit: &'static str,
    pub active: &'static str,
    pub expired: &'static str,
    pub no_accounts: &'static str,
    pub no_accounts_hint: &'static str,
    pub save_current: &'static str,
    pub save_failed: &'static str,
    pub remove_account: &'static str,
    pub remove_confirm: &'static str,
    pub switch_failed: &'static str,
    pub session_expired_title: &'static str,
    pub session_expired: &'static str,
    pub err_no_launcher_data: &'static str,
    pub err_no_session: &'static str,
    pub err_no_account_id: &'static str,
}

/// Substitute `{name}` in a template label.
pub fn with_name(template: &str, name: &str) -> String {
    template.replace("{name}", name)
}

pub fn labels(lang: &str) -> Labels {
    if lang == "de" {
        Labels {
            settings: "Einstellungen",
            language: "Sprache",
            autostart: "Mit Windows starten",
            display_name: "Angezeigter Name",
            name_display: "Epic-Name",
            name_id: "Konto-ID",
            quit: "Beenden",
            active: "aktiv",
            expired: "abgelaufen",
            no_accounts: "Noch keine Konten gespeichert",
            no_accounts_hint: "In Epic einloggen, dann \u{201e}Aktuelles Konto speichern\u{201c}",
            save_current: "Aktuelles Konto speichern",
            save_failed: "Konto konnte nicht gespeichert werden",
            remove_account: "Konto entfernen",
            remove_confirm: "{name} aus dem Switcher entfernen?\n\nDas Epic-Konto selbst bleibt unber\u{fc}hrt.",
            switch_failed: "Konto konnte nicht gewechselt werden",
            session_expired_title: "Sitzung abgelaufen",
            session_expired: "Epic hat {name} abgemeldet \u{2014} die gespeicherte Sitzung ist abgelaufen.\n\nIm Epic Games Launcher neu einloggen (mit \u{201e}Angemeldet bleiben\u{201c}) und dann \u{201e}Aktuelles Konto speichern\u{201c} klicken.",
            err_no_launcher_data: "Epic Games Launcher-Daten nicht gefunden. Starte den Launcher einmal und logge dich ein.",
            err_no_session: "Keine aktive Epic-Sitzung gefunden. Logge dich im Epic Games Launcher mit aktiviertem \u{201e}Angemeldet bleiben\u{201c} ein und versuche es erneut.",
            err_no_account_id: "Die Konto-ID konnte nicht ermittelt werden. Starte den Epic Games Launcher einmal eingeloggt und versuche es erneut.",
        }
    } else {
        Labels {
            settings: "Settings",
            language: "Language",
            autostart: "Start with Windows",
            display_name: "Display name",
            name_display: "Epic name",
            name_id: "Account ID",
            quit: "Quit",
            active: "active",
            expired: "expired",
            no_accounts: "No accounts saved yet",
            no_accounts_hint: "Log in to Epic, then \u{201c}Save current account\u{201d}",
            save_current: "Save current account",
            save_failed: "Couldn't save account",
            remove_account: "Remove account",
            remove_confirm: "Remove {name} from the switcher?\n\nThe Epic account itself is not affected.",
            switch_failed: "Couldn't switch account",
            session_expired_title: "Session expired",
            session_expired: "Epic signed {name} out \u{2014} the saved session has expired.\n\nLog in again in the Epic Games Launcher (with \u{201c}Remember me\u{201d}) and click \u{201c}Save current account\u{201d}.",
            err_no_launcher_data: "Epic Games Launcher data not found. Start the launcher once and log in first.",
            err_no_session: "No active Epic session found. Log in to the Epic Games Launcher with \u{201c}Remember me\u{201d} enabled, then try again.",
            err_no_account_id: "Couldn't determine the account ID. Start the Epic Games Launcher once while logged in, then try again.",
        }
    }
}
