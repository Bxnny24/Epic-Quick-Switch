//! Extracting account display names from Epic Games Launcher logs.
//!
//! The launcher passes `-epicusername="..."` and `-epicuserid=...` as game
//! launch arguments and logs the full command line. That log line is the only
//! local source for the human-readable username, so names are best-effort:
//! they exist only if a game was launched, and logs rotate away. Callers must
//! be able to fall back to the account ID.

use std::path::{Path, PathBuf};

use crate::epic::paths;

/// An account identity as found in a launcher log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogIdentity {
    pub account_id: String,
    pub username: String,
}

/// The username logged for a specific account ID, newest occurrence first.
pub fn username_for(account_id: &str) -> Option<String> {
    let dir = paths::logs_dir()?;
    for file in log_files_newest_first(&dir) {
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let found = text
            .lines()
            .rev()
            .filter_map(extract_identity)
            .find(|i| i.account_id.eq_ignore_ascii_case(account_id));
        if let Some(identity) = found {
            return Some(identity.username);
        }
    }
    None
}

/// All `*EpicGamesLauncher*.log` files in `dir`, newest modified first.
fn log_files_newest_first(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.contains("EpicGamesLauncher") && name.ends_with(".log")
        })
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((modified, p))
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Parse `-epicusername="<name>"` and `-epicuserid=<hex>` out of one log line.
/// Both markers must be present for a match.
fn extract_identity(line: &str) -> Option<LogIdentity> {
    let username_start = line.find("-epicusername=\"")? + "-epicusername=\"".len();
    let username_end = line[username_start..].find('"')? + username_start;
    let username = &line[username_start..username_end];

    let id_start = line.find("-epicuserid=")? + "-epicuserid=".len();
    let account_id: String = line[id_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();

    if username.is_empty() || account_id.is_empty() {
        return None;
    }
    Some(LogIdentity { account_id, username: username.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH_LINE: &str = r#"[2026.07.12-10.00.00:000][  0]LogPortalLaunch: FCommunityPortalLaunchAppTask: Preparing to launch app with commandline: -AUTH_LOGIN=unused -epicapp=Fortnite -epicenv=Prod -epicusername="Benny Test" -epicuserid=a1b2c3d4e5f60718293a4b5c6d7e8f90 -epiclocale=de"#;

    #[test]
    fn identity_is_extracted_from_launch_line() {
        let identity = extract_identity(LAUNCH_LINE).expect("identity");
        assert_eq!(identity.username, "Benny Test");
        assert_eq!(identity.account_id, "a1b2c3d4e5f60718293a4b5c6d7e8f90");
    }

    #[test]
    fn missing_markers_yield_none() {
        assert_eq!(extract_identity("LogInit: some unrelated line"), None);
        assert_eq!(extract_identity(r#"-epicusername="OnlyName""#), None);
        assert_eq!(extract_identity("-epicuserid=abc123"), None);
    }

    #[test]
    fn malformed_quote_yields_none() {
        assert_eq!(
            extract_identity(r#"-epicusername="unterminated -epicuserid=abc123"#),
            None
        );
    }

    #[test]
    fn empty_values_yield_none() {
        assert_eq!(
            extract_identity(r#"-epicusername="" -epicuserid=abc123"#),
            None
        );
        assert_eq!(
            extract_identity(r#"-epicusername="Name" -epicuserid= -other"#),
            None
        );
    }

    #[test]
    fn unicode_usernames_are_supported() {
        let line = r#"-epicusername="Bënny Ümläut" -epicuserid=ff00aa11bb22cc33dd44ee55ff667788"#;
        let identity = extract_identity(line).expect("identity");
        assert_eq!(identity.username, "Bënny Ümläut");
    }

    #[test]
    fn newest_log_file_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = dir.path().join("EpicGamesLauncher-backup-old.log");
        let new = dir.path().join("EpicGamesLauncher.log");
        let unrelated = dir.path().join("UnrealEngine.log");
        std::fs::write(&old, "old").expect("write");
        std::fs::write(&new, "new").expect("write");
        std::fs::write(&unrelated, "x").expect("write");

        // Backdate the old file so ordering is deterministic.
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        filetime_set(&old, past);

        let files = log_files_newest_first(dir.path());
        assert_eq!(files.len(), 2, "unrelated log must be filtered out");
        assert_eq!(files[0], new);
        assert_eq!(files[1], old);
    }

    /// Minimal mtime setter (no extra dev-dependency).
    fn filetime_set(path: &Path, time: std::time::SystemTime) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_modified(time).expect("set mtime");
    }
}
