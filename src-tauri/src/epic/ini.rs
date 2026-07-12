//! Minimal, encoding-preserving editor for the launcher's `GameUserSettings.ini`.
//!
//! Only the `[RememberMe]` section is ever touched. Everything else — encoding
//! (UTF-8 or UTF-16 LE, with or without BOM), line terminators, duplicate keys
//! in other sections — round-trips byte-identically. This matters: Unreal
//! Engine writes some configs as UTF-16 LE, and rewriting the file in the
//! wrong encoding silently logs the user out of everything.

use std::fs;
use std::path::Path;

/// `Data=` blobs shorter than this are placeholders left behind by a logout,
/// not a usable session token.
pub const MIN_DATA_LEN: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IniEncoding {
    Utf8 { bom: bool },
    Utf16Le { bom: bool },
}

/// A loaded ini preserved losslessly: raw lines (without terminators), the
/// original encoding, and the dominant line terminator.
pub struct IniFile {
    pub lines: Vec<String>,
    pub encoding: IniEncoding,
    pub newline: &'static str,
    /// Whether the file ended with a line terminator (preserved on save).
    pub trailing_newline: bool,
}

/// The `[RememberMe]` section as read from the ini.
#[derive(Debug, Clone)]
pub struct RememberMe {
    pub enabled: bool,
    pub data: Option<String>,
}

impl RememberMe {
    /// A session that can actually log in: enabled and a real token blob.
    pub fn is_valid(&self) -> bool {
        self.enabled && self.data.as_deref().is_some_and(|d| d.len() > MIN_DATA_LEN)
    }
}

pub fn load(path: &Path) -> Result<IniFile, String> {
    let bytes =
        fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let (text, encoding) = decode(&bytes)?;
    Ok(from_text(&text, encoding))
}

/// Write the ini back atomically (temp file + rename) in its original
/// encoding, BOM and line terminator.
pub fn save(path: &Path, file: &IniFile) -> Result<(), String> {
    let mut text = file.lines.join(file.newline);
    if file.trailing_newline {
        text.push_str(file.newline);
    }
    let bytes = encode(&text, file.encoding);

    let tmp = path.with_extension("ini.eqs-tmp");
    fs::write(&tmp, &bytes).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("failed to replace {}: {e}", path.display())
    })
}

/// Read the `[RememberMe]` section. `None` if the section does not exist.
pub fn read_remember_me(file: &IniFile) -> Option<RememberMe> {
    let (start, end) = section_bounds(&file.lines, "RememberMe")?;
    let mut remember = RememberMe { enabled: false, data: None };
    for line in &file.lines[start + 1..end] {
        if let Some(value) = key_value(line, "Enable") {
            remember.enabled = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = key_value(line, "Data") {
            if !value.is_empty() {
                remember.data = Some(value.to_string());
            }
        }
    }
    Some(remember)
}

/// Patch the `[RememberMe]` section in place: replace the first `Enable=` and
/// `Data=` lines (creating missing keys, or the whole section at EOF). Every
/// other line is left untouched.
pub fn write_remember_me(file: &mut IniFile, enabled: bool, data: &str) {
    let enable_line = format!("Enable={}", if enabled { "True" } else { "False" });
    let data_line = format!("Data={data}");

    if let Some((start, end)) = section_bounds(&file.lines, "RememberMe") {
        let mut enable_done = false;
        let mut data_done = false;
        for line in &mut file.lines[start + 1..end] {
            if !enable_done && key_value(line, "Enable").is_some() {
                *line = enable_line.clone();
                enable_done = true;
            } else if !data_done && key_value(line, "Data").is_some() {
                *line = data_line.clone();
                data_done = true;
            }
        }
        // Insert missing keys right after the last non-blank line of the
        // section, keeping any blank separator lines below.
        let mut insert_at = start + 1;
        for (i, line) in file.lines[start + 1..end].iter().enumerate() {
            if !line.trim().is_empty() {
                insert_at = start + 1 + i + 1;
            }
        }
        if !data_done {
            file.lines.insert(insert_at, data_line);
        }
        if !enable_done {
            file.lines.insert(start + 1, enable_line);
        }
    } else {
        if file.lines.last().is_some_and(|l| !l.trim().is_empty()) {
            file.lines.push(String::new());
        }
        file.lines.push("[RememberMe]".to_string());
        file.lines.push(enable_line);
        file.lines.push(data_line);
        file.trailing_newline = true;
    }
}

/// Whether the file at `path` contains a `[RememberMe]` section (used to pick
/// between candidate ini locations).
pub fn has_remember_me(path: &Path) -> bool {
    let Ok(file) = load(path) else {
        return false;
    };
    read_remember_me(&file).is_some()
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn from_text(text: &str, encoding: IniEncoding) -> IniFile {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    if trailing_newline {
        lines.pop();
    }
    IniFile { lines, encoding, newline, trailing_newline }
}

fn decode(bytes: &[u8]) -> Result<(String, IniEncoding), String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((decode_utf16_le(rest)?, IniEncoding::Utf16Le { bom: true }));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let text = std::str::from_utf8(rest).map_err(|e| format!("invalid UTF-8: {e}"))?;
        return Ok((text.to_string(), IniEncoding::Utf8 { bom: true }));
    }
    // Text files never contain NUL bytes in UTF-8; their presence means
    // BOM-less UTF-16 LE (every ASCII char has a high zero byte).
    if bytes.contains(&0) {
        return Ok((decode_utf16_le(bytes)?, IniEncoding::Utf16Le { bom: false }));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))?;
    Ok((text.to_string(), IniEncoding::Utf8 { bom: false }))
}

fn decode_utf16_le(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() % 2 != 0 {
        return Err("invalid UTF-16: odd byte length".to_string());
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).map_err(|e| format!("invalid UTF-16: {e}"))
}

fn encode(text: &str, encoding: IniEncoding) -> Vec<u8> {
    match encoding {
        IniEncoding::Utf8 { bom } => {
            let mut out = Vec::with_capacity(text.len() + 3);
            if bom {
                out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            }
            out.extend_from_slice(text.as_bytes());
            out
        }
        IniEncoding::Utf16Le { bom } => {
            let mut out = Vec::with_capacity(text.len() * 2 + 2);
            if bom {
                out.extend_from_slice(&[0xFF, 0xFE]);
            }
            for unit in text.encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out
        }
    }
}

/// `(header_index, end_exclusive)` of the named section: the header line and
/// the index of the next section header (or EOF).
fn section_bounds(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let header = format!("[{name}]");
    let start = lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(&header))?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    Some((start, end))
}

/// The trimmed value if `line` is `key=value` for the given key (ASCII
/// case-insensitive), else `None`.
fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once('=')?;
    if k.trim().eq_ignore_ascii_case(key) {
        Some(v.trim())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Launcher]\r\n+Entry=one\r\n+Entry=two\r\n\r\n[RememberMe]\r\nEnable=True\r\nData=abc123\r\n\r\n[Other]\r\nKey=Value\r\n";

    fn utf16_le_bytes(text: &str, bom: bool) -> Vec<u8> {
        encode(text, IniEncoding::Utf16Le { bom })
    }

    fn roundtrip(bytes: &[u8]) -> Vec<u8> {
        let (text, encoding) = decode(bytes).expect("decode");
        let file = from_text(&text, encoding);
        let mut out = file.lines.join(file.newline);
        if file.trailing_newline {
            out.push_str(file.newline);
        }
        encode(&out, file.encoding)
    }

    #[test]
    fn utf8_roundtrip_is_byte_identical() {
        assert_eq!(roundtrip(SAMPLE.as_bytes()), SAMPLE.as_bytes());
    }

    #[test]
    fn utf8_bom_roundtrip_is_byte_identical() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(SAMPLE.as_bytes());
        assert_eq!(roundtrip(&bytes), bytes);
    }

    #[test]
    fn utf16_le_bom_roundtrip_is_byte_identical() {
        let bytes = utf16_le_bytes(SAMPLE, true);
        assert_eq!(roundtrip(&bytes), bytes);
    }

    #[test]
    fn bomless_utf16_le_is_detected_and_roundtrips() {
        let bytes = utf16_le_bytes(SAMPLE, false);
        let (_, encoding) = decode(&bytes).expect("decode");
        assert_eq!(encoding, IniEncoding::Utf16Le { bom: false });
        assert_eq!(roundtrip(&bytes), bytes);
    }

    #[test]
    fn lf_only_files_keep_lf() {
        let sample = SAMPLE.replace("\r\n", "\n");
        assert_eq!(roundtrip(sample.as_bytes()), sample.as_bytes());
    }

    #[test]
    fn file_without_trailing_newline_roundtrips() {
        let sample = SAMPLE.trim_end();
        assert_eq!(roundtrip(sample.as_bytes()), sample.as_bytes());
    }

    #[test]
    fn read_remember_me_parses_section() {
        let file = from_text(SAMPLE, IniEncoding::Utf8 { bom: false });
        let rm = read_remember_me(&file).expect("section exists");
        assert!(rm.enabled);
        assert_eq!(rm.data.as_deref(), Some("abc123"));
    }

    #[test]
    fn read_remember_me_handles_logout_state() {
        let text = "[RememberMe]\nEnable=False\nData=\n";
        let file = from_text(text, IniEncoding::Utf8 { bom: false });
        let rm = read_remember_me(&file).expect("section exists");
        assert!(!rm.enabled);
        assert_eq!(rm.data, None);
        assert!(!rm.is_valid());
    }

    #[test]
    fn read_remember_me_is_none_without_section() {
        let file = from_text("[Other]\nKey=Value\n", IniEncoding::Utf8 { bom: false });
        assert!(read_remember_me(&file).is_none());
    }

    #[test]
    fn keys_and_section_match_case_insensitively() {
        let text = "[rememberme]\nenable=true\ndata=xyz\n";
        let file = from_text(text, IniEncoding::Utf8 { bom: false });
        let rm = read_remember_me(&file).expect("section exists");
        assert!(rm.enabled);
        assert_eq!(rm.data.as_deref(), Some("xyz"));
    }

    #[test]
    fn is_valid_requires_long_data() {
        let short = RememberMe { enabled: true, data: Some("x".repeat(10)) };
        let long = RememberMe { enabled: true, data: Some("x".repeat(MIN_DATA_LEN + 1)) };
        let disabled = RememberMe { enabled: false, data: Some("x".repeat(MIN_DATA_LEN + 1)) };
        assert!(!short.is_valid());
        assert!(long.is_valid());
        assert!(!disabled.is_valid());
    }

    #[test]
    fn write_replaces_only_remember_me_lines() {
        let mut file = from_text(SAMPLE, IniEncoding::Utf8 { bom: false });
        write_remember_me(&mut file, true, "NEWBLOB");
        let out = file.lines.join("\n");
        assert!(out.contains("Data=NEWBLOB"));
        assert!(!out.contains("Data=abc123"));
        // Untouched sections stay byte-identical.
        assert!(out.contains("+Entry=one"));
        assert!(out.contains("[Other]\nKey=Value"));
        // Still exactly one RememberMe section.
        assert_eq!(out.matches("[RememberMe]").count(), 1);
    }

    #[test]
    fn write_creates_missing_keys_in_section() {
        let text = "[RememberMe]\n\n[Other]\nKey=Value\n";
        let mut file = from_text(text, IniEncoding::Utf8 { bom: false });
        write_remember_me(&mut file, true, "BLOB");
        let rm = read_remember_me(&file).expect("section exists");
        assert!(rm.enabled);
        assert_eq!(rm.data.as_deref(), Some("BLOB"));
        // Blank separator stays below the inserted keys.
        let out = file.lines.join("\n");
        assert!(out.contains("[RememberMe]\nEnable=True\nData=BLOB\n\n[Other]"));
    }

    #[test]
    fn write_appends_section_when_missing() {
        let text = "[Other]\nKey=Value\n";
        let mut file = from_text(text, IniEncoding::Utf8 { bom: false });
        write_remember_me(&mut file, true, "BLOB");
        let out = file.lines.join("\n");
        assert!(out.ends_with("[RememberMe]\nEnable=True\nData=BLOB"));
        assert!(file.trailing_newline);
        // Original content untouched.
        assert!(out.starts_with("[Other]\nKey=Value"));
    }

    #[test]
    fn write_to_empty_file_creates_section() {
        let mut file = from_text("", IniEncoding::Utf8 { bom: false });
        write_remember_me(&mut file, false, "");
        let rm = read_remember_me(&file).expect("section exists");
        assert!(!rm.enabled);
        assert_eq!(rm.data, None);
    }

    #[test]
    fn save_and_load_roundtrip_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("GameUserSettings.ini");
        let bytes = utf16_le_bytes(SAMPLE, true);
        std::fs::write(&path, &bytes).expect("write fixture");

        let mut file = load(&path).expect("load");
        write_remember_me(&mut file, true, "PATCHED");
        save(&path, &file).expect("save");

        let reloaded = load(&path).expect("reload");
        assert_eq!(reloaded.encoding, IniEncoding::Utf16Le { bom: true });
        let rm = read_remember_me(&reloaded).expect("section");
        assert_eq!(rm.data.as_deref(), Some("PATCHED"));
        // No temp litter left behind.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
    }
}
