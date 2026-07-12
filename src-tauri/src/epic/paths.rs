//! Locating Epic Games Launcher files: the session ini, the log directory,
//! and the launcher executable.

use std::path::{Path, PathBuf};

use crate::epic::{ini, registry};

/// Both known locations of `GameUserSettings.ini`, in preference order.
/// Current launcher builds write to `WindowsEditor` (the launcher is an
/// Unreal-editor-derived app); some builds/guides reference `Windows`.
pub fn ini_candidates() -> Vec<PathBuf> {
    let Some(local) = local_app_data() else {
        return Vec::new();
    };
    let config = local.join("EpicGamesLauncher").join("Saved").join("Config");
    vec![
        config.join("WindowsEditor").join("GameUserSettings.ini"),
        config.join("Windows").join("GameUserSettings.ini"),
    ]
}

/// The resolved session ini location(s).
pub struct IniLocation {
    /// The authoritative ini all reads use.
    pub primary: PathBuf,
    /// The sibling candidate, if it also exists. Writes are mirrored there so
    /// whichever file this launcher build actually reads never keeps a stale
    /// session (logging into the wrong account would be the worst failure).
    pub mirror: Option<PathBuf>,
}

/// Pick the session ini: of the candidates that exist, prefer the one with a
/// `[RememberMe]` section; if both (or neither) qualify, the newer one wins.
pub fn resolve_ini() -> Result<IniLocation, String> {
    let existing: Vec<PathBuf> = ini_candidates().into_iter().filter(|p| p.exists()).collect();
    match existing.len() {
        0 => Err(
            "Epic Games Launcher config not found. Start the launcher once and log in first."
                .to_string(),
        ),
        1 => Ok(IniLocation { primary: existing.into_iter().next().unwrap(), mirror: None }),
        _ => {
            let (a, b) = (existing[0].clone(), existing[1].clone());
            let (primary, mirror) = match (ini::has_remember_me(&a), ini::has_remember_me(&b)) {
                (true, false) => (a, b),
                (false, true) => (b, a),
                _ => {
                    if modified(&b) > modified(&a) {
                        (b, a)
                    } else {
                        (a, b)
                    }
                }
            };
            Ok(IniLocation { primary, mirror: Some(mirror) })
        }
    }
}

/// `%LOCALAPPDATA%\EpicGamesLauncher\Saved\Logs`
pub fn logs_dir() -> Option<PathBuf> {
    Some(local_app_data()?.join("EpicGamesLauncher").join("Saved").join("Logs"))
}

/// The launcher executable, resolved fresh on every call (self-updates can
/// move the binary):
///   1. the `com.epicgames.launcher` URL-protocol handler command (registry),
///   2. the default install paths (Win64, then Win32).
pub fn launcher_exe() -> Option<PathBuf> {
    if let Some(command) = registry::launcher_open_command() {
        if let Some(exe) = exe_from_open_command(&command) {
            if exe.exists() {
                return Some(exe);
            }
        }
    }

    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Ok(programs) = std::env::var(var) else {
            continue;
        };
        let binaries = PathBuf::from(programs)
            .join("Epic Games")
            .join("Launcher")
            .join("Portal")
            .join("Binaries");
        for arch in ["Win64", "Win32"] {
            let exe = binaries.join(arch).join("EpicGamesLauncher.exe");
            if exe.exists() {
                return Some(exe);
            }
        }
    }

    None
}

fn local_app_data() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Extract the executable path from a `shell\open\command` value such as
/// `"C:\...\EpicGamesLauncher.exe" %1` (quoted) or an unquoted variant.
fn exe_from_open_command(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let exe = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        command.split_whitespace().next()?
    };
    if exe.is_empty() {
        None
    } else {
        Some(PathBuf::from(exe))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_is_extracted_from_quoted_command() {
        let cmd = r#""C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe" "%1""#;
        assert_eq!(
            exe_from_open_command(cmd),
            Some(PathBuf::from(
                r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"
            ))
        );
    }

    #[test]
    fn exe_is_extracted_from_unquoted_command() {
        let cmd = r"C:\Epic\EpicGamesLauncher.exe %1";
        assert_eq!(
            exe_from_open_command(cmd),
            Some(PathBuf::from(r"C:\Epic\EpicGamesLauncher.exe"))
        );
    }

    #[test]
    fn empty_command_yields_none() {
        assert_eq!(exe_from_open_command(""), None);
        assert_eq!(exe_from_open_command("   "), None);
        assert_eq!(exe_from_open_command("\"\""), None);
    }

    /// Smoke test against the real machine. Ignored by default (needs Epic).
    /// Run with: cargo test -- --ignored --nocapture print_resolved_paths
    #[test]
    #[ignore]
    fn print_resolved_paths() {
        println!("candidates: {:#?}", ini_candidates());
        match resolve_ini() {
            Ok(loc) => {
                println!("primary: {}", loc.primary.display());
                println!("mirror:  {:?}", loc.mirror);
            }
            Err(e) => println!("resolve_ini: {e}"),
        }
        println!("logs dir: {:?}", logs_dir());
        println!("launcher exe: {:?}", launcher_exe());
    }
}
