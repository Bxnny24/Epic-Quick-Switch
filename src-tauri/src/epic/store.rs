//! The app's own store of Epic account session snapshots.
//!
//! Epic's launcher holds only ONE login session at a time, so switching
//! requires keeping a copy of each account's `[RememberMe]` token here:
//! `accounts.json` in the app data directory, written atomically.
//!
//! Security: on disk the token blobs are wrapped with DPAPI (current-user
//! scope), so the file is useless to other Windows users or off-machine
//! copies. Note the launcher's own `GameUserSettings.ini` keeps the same
//! token in plaintext — this store never weakens the machine's posture.
//! The blob must NEVER be logged; `EpicAccount`'s `Debug` impl redacts it.

use std::fmt;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const STORE_FILE: &str = "accounts.json";
const STORE_VERSION: u32 = 1;
/// Marks a DPAPI-wrapped, base64-encoded blob on disk. Values without the
/// prefix are treated as plaintext (manual recovery / migration path).
const DPAPI_PREFIX: &str = "dpapi:";

/// Serializes every load-mutate-save sequence on the account store across the
/// tray's worker threads (and the whole switch), so concurrent actions cannot
/// clobber each other's writes with a stale full-list snapshot. The
/// single-instance plugin guarantees one process, so a process-wide lock is
/// enough. Hold it around a load…save critical section (see [`lock`]).
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the store lock, recovering from poisoning — a panic inside one
/// critical section must not wedge every later store operation.
pub fn lock() -> MutexGuard<'static, ()> {
    STORE_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// One saved account. `remember_me_data` is held decrypted in memory.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicAccount {
    pub account_id: String,
    pub display_name: String,
    #[serde(default)]
    pub display_name_is_custom: bool,
    pub remember_me_data: String,
    pub saved_at: u64,
    #[serde(default)]
    pub last_used: Option<u64>,
    /// Set when a post-switch check saw the launcher reject the token.
    #[serde(default)]
    pub stale: bool,
    /// The original on-disk wrapped blob when it could not be decrypted on
    /// load (transient DPAPI failure, wrong user/machine). Kept in memory,
    /// never serialized, and written back verbatim so an unrelated save does
    /// not destroy ciphertext that might decrypt fine later. Cleared once the
    /// account is re-captured.
    #[serde(skip)]
    undecryptable_raw: Option<String>,
}

impl fmt::Debug for EpicAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EpicAccount")
            .field("account_id", &self.account_id)
            .field("display_name", &self.display_name)
            .field("remember_me_data", &format!("<redacted, len {}>", self.remember_me_data.len()))
            .field("saved_at", &self.saved_at)
            .field("last_used", &self.last_used)
            .field("stale", &self.stale)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreFileFormat {
    version: u32,
    accounts: Vec<EpicAccount>,
    /// Tombstones for explicitly removed accounts (lowercased ids). The
    /// switch's outgoing auto-save consults this so a deliberately deleted
    /// token is not silently re-captured; a manual "save current account"
    /// clears the tombstone.
    #[serde(default)]
    removed_ids: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpsertOutcome {
    Added,
    Updated,
}

pub struct AccountStore {
    path: PathBuf,
    accounts: Vec<EpicAccount>,
    removed_ids: Vec<String>,
}

impl AccountStore {
    /// Load the store from the app data directory. A missing file yields an
    /// empty store; a corrupt file is renamed to `accounts.json.bad` (never
    /// silently overwritten) and an empty store is returned.
    pub fn load(app: &AppHandle) -> AccountStore {
        let path = store_path(app);
        Self::load_from(path)
    }

    fn load_from(path: PathBuf) -> AccountStore {
        let mut store = AccountStore { path, accounts: Vec::new(), removed_ids: Vec::new() };
        let Ok(bytes) = std::fs::read(&store.path) else {
            return store;
        };
        match serde_json::from_slice::<StoreFileFormat>(&bytes) {
            Ok(file) => {
                store.removed_ids = file.removed_ids;
                store.accounts = file
                    .accounts
                    .into_iter()
                    .map(|mut account| {
                        match unwrap_data(&account.remember_me_data) {
                            Ok(plain) => account.remember_me_data = plain,
                            Err(_) => {
                                // Undecryptable (transient DPAPI failure, other
                                // machine/user): blank the usable token so the
                                // account is unusable-until-re-saved, but keep
                                // the original ciphertext so a later save does
                                // not destroy data that might decrypt fine.
                                account.undecryptable_raw =
                                    Some(std::mem::take(&mut account.remember_me_data));
                                account.stale = true;
                            }
                        }
                        account
                    })
                    .collect();
            }
            Err(_) => {
                // Quarantine to the first free numbered name: Windows rename
                // replaces existing files, and overwriting an earlier .bad
                // would silently destroy previously quarantined tokens.
                let mut bad = store.path.with_extension("json.bad");
                let mut n = 1;
                while bad.exists() && n <= 100 {
                    bad = store.path.with_extension(format!("json.bad.{n}"));
                    n += 1;
                }
                let _ = std::fs::rename(&store.path, bad);
            }
        }
        store
    }

    /// Persist atomically (temp file + rename), DPAPI-wrapping each blob.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let on_disk = StoreFileFormat {
            version: STORE_VERSION,
            removed_ids: self.removed_ids.clone(),
            accounts: self
                .accounts
                .iter()
                .cloned()
                .map(|mut account| {
                    // An entry whose blob never decrypted this session keeps
                    // its original ciphertext; everything else is (re)wrapped.
                    account.remember_me_data = match account.undecryptable_raw.take() {
                        Some(raw) => raw,
                        None => wrap_data(&account.remember_me_data)?,
                    };
                    Ok(account)
                })
                .collect::<Result<Vec<_>, String>>()?,
        };
        let json = serde_json::to_vec_pretty(&on_disk)
            .map_err(|e| format!("failed to serialize account store: {e}"))?;

        // Durable atomic replace: flush the temp file to disk before the
        // rename so a crash cannot leave a zero-length/torn accounts.json in
        // place (which would lose every stored token at once).
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)
                .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
            file.write_all(&json)
                .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
            file.sync_all()
                .map_err(|e| format!("failed to flush {}: {e}", tmp.display()))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("failed to replace {}: {e}", self.path.display())
        })
    }

    pub fn accounts(&self) -> &[EpicAccount] {
        &self.accounts
    }

    pub fn get(&self, account_id: &str) -> Option<&EpicAccount> {
        self.accounts
            .iter()
            .find(|a| a.account_id.eq_ignore_ascii_case(account_id))
    }

    /// Insert or refresh a session snapshot. Re-saving an account updates its
    /// blob and clears `stale`; a user-renamed display name is preserved, a
    /// generated one is upgraded when the logs finally yield a real username.
    pub fn upsert_session(
        &mut self,
        account_id: &str,
        data: String,
        log_name: Option<String>,
    ) -> UpsertOutcome {
        let now = now_secs();
        // Saving an account is an explicit decision to keep it — lift any
        // removal tombstone.
        self.removed_ids.retain(|id| !id.eq_ignore_ascii_case(account_id));
        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|a| a.account_id.eq_ignore_ascii_case(account_id))
        {
            existing.remember_me_data = data;
            existing.saved_at = now;
            existing.stale = false;
            existing.undecryptable_raw = None;
            if !existing.display_name_is_custom {
                if let Some(name) = log_name {
                    existing.display_name = name;
                }
            }
            UpsertOutcome::Updated
        } else {
            let display_name = log_name.unwrap_or_else(|| fallback_name(account_id));
            self.accounts.push(EpicAccount {
                account_id: account_id.to_string(),
                display_name,
                display_name_is_custom: false,
                remember_me_data: data,
                saved_at: now,
                last_used: None,
                stale: false,
                undecryptable_raw: None,
            });
            UpsertOutcome::Added
        }
    }

    pub fn remove(&mut self, account_id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts
            .retain(|a| !a.account_id.eq_ignore_ascii_case(account_id));
        let removed = self.accounts.len() != before;
        // Tombstone: the switch's outgoing auto-save must not silently
        // re-capture a token the user explicitly deleted.
        if removed && !self.is_removed(account_id) {
            self.removed_ids.push(account_id.to_lowercase());
        }
        removed
    }

    /// Whether this account was explicitly removed by the user (and not
    /// re-saved since). Consulted by the switch's outgoing auto-save.
    pub fn is_removed(&self, account_id: &str) -> bool {
        self.removed_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(account_id))
    }

    pub fn touch_last_used(&mut self, account_id: &str) {
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.account_id.eq_ignore_ascii_case(account_id))
        {
            account.last_used = Some(now_secs());
        }
    }

    /// Best-effort upgrade of a saved account's display name from a name that
    /// was already resolved for THIS account (looked up by its own ID). A
    /// `None` name is a no-op, so an existing name is never cleared, and a
    /// user rename (`display_name_is_custom`) is never overwritten. Unlike
    /// `upsert_session` it touches neither the token nor `saved_at` — the
    /// switch uses it to keep the target's name fresh without re-capturing.
    pub fn refresh_log_name(&mut self, account_id: &str, log_name: Option<String>) {
        let Some(name) = log_name else {
            return;
        };
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.account_id.eq_ignore_ascii_case(account_id))
        {
            if !account.display_name_is_custom {
                account.display_name = name;
            }
        }
    }

    pub fn mark_stale(&mut self, account_id: &str) {
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.account_id.eq_ignore_ascii_case(account_id))
        {
            account.stale = true;
        }
    }
}

/// `%AppData%\<identifier>\accounts.json`
fn store_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join(STORE_FILE))
        .unwrap_or_else(|_| PathBuf::from(STORE_FILE))
}

/// "Account " + the first 8 characters of the ID — used until the launcher
/// logs yield a real username.
fn fallback_name(account_id: &str) -> String {
    let short: String = account_id.chars().take(8).collect();
    format!("Account {short}")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// DPAPI (current-user scope) via crypt32 — no extra dependency, matching the
// app's raw-FFI style for user32 dialogs.
// ---------------------------------------------------------------------------

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        p_data_in: *const DataBlob,
        sz_data_descr: *const u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut core::ffi::c_void,
        p_prompt_struct: *mut core::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;
    fn CryptUnprotectData(
        p_data_in: *const DataBlob,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut core::ffi::c_void,
        p_prompt_struct: *mut core::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(h_mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;
/// App-specific entropy: binds the ciphertext to this app on top of the
/// per-user DPAPI key. Changing it invalidates every stored blob.
const ENTROPY: &[u8] = b"epic-quick-switch:v1";

fn entropy_blob() -> DataBlob {
    DataBlob { cb_data: ENTROPY.len() as u32, pb_data: ENTROPY.as_ptr() as *mut u8 }
}

fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    let input = DataBlob { cb_data: plain.len() as u32, pb_data: plain.as_ptr() as *mut u8 };
    let entropy = entropy_blob();
    let mut output = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
    // SAFETY: all blobs point at live buffers for the duration of the call;
    // on success the output buffer is copied out and freed via LocalFree.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &entropy,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("DPAPI encryption failed".to_string());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) }.to_vec();
    unsafe { LocalFree(output.pb_data as *mut core::ffi::c_void) };
    Ok(bytes)
}

fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    let input = DataBlob { cb_data: cipher.len() as u32, pb_data: cipher.as_ptr() as *mut u8 };
    let entropy = entropy_blob();
    let mut output = DataBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
    // SAFETY: see dpapi_protect.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("DPAPI decryption failed".to_string());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) }.to_vec();
    unsafe { LocalFree(output.pb_data as *mut core::ffi::c_void) };
    Ok(bytes)
}

/// Plain blob → `dpapi:<base64>` for disk. Empty stays empty.
fn wrap_data(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    let cipher = dpapi_protect(plain.as_bytes())?;
    Ok(format!("{DPAPI_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(cipher)))
}

/// Disk value → plain blob. Values without the prefix pass through verbatim.
fn unwrap_data(stored: &str) -> Result<String, String> {
    let Some(encoded) = stored.strip_prefix(DPAPI_PREFIX) else {
        return Ok(stored.to_string());
    };
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid base64 in store: {e}"))?;
    let plain = dpapi_unprotect(&cipher)?;
    String::from_utf8(plain).map_err(|e| format!("invalid UTF-8 in store blob: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, AccountStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AccountStore::load_from(dir.path().join(STORE_FILE));
        (dir, store)
    }

    #[test]
    fn missing_file_loads_empty() {
        let (_dir, store) = temp_store();
        assert!(store.accounts().is_empty());
    }

    #[test]
    fn save_load_roundtrip_encrypts_on_disk() {
        let (dir, mut store) = temp_store();
        let blob = "x".repeat(1500);
        store.upsert_session("abc123", blob.clone(), Some("Benny".to_string()));
        store.save().expect("save");

        // On disk: wrapped, not plaintext.
        let raw = std::fs::read_to_string(dir.path().join(STORE_FILE)).expect("read");
        assert!(raw.contains("dpapi:"));
        assert!(!raw.contains(&blob));

        // Reload: decrypted back to the original.
        let reloaded = AccountStore::load_from(dir.path().join(STORE_FILE));
        let account = reloaded.get("abc123").expect("account");
        assert_eq!(account.remember_me_data, blob);
        assert_eq!(account.display_name, "Benny");
        assert!(!account.stale);
        // Atomic write leaves no temp litter.
        assert!(!dir.path().join("accounts.json.tmp").exists());
    }

    #[test]
    fn upsert_updates_existing_and_preserves_custom_name() {
        let (_dir, mut store) = temp_store();
        assert_eq!(
            store.upsert_session("abc", "one".into(), None),
            UpsertOutcome::Added
        );
        assert_eq!(store.get("abc").unwrap().display_name, "Account abc");

        // Log name upgrades a generated name.
        assert_eq!(
            store.upsert_session("ABC", "two".into(), Some("RealName".into())),
            UpsertOutcome::Updated
        );
        assert_eq!(store.accounts().len(), 1, "case-insensitive dedupe");
        assert_eq!(store.get("abc").unwrap().display_name, "RealName");
        assert_eq!(store.get("abc").unwrap().remember_me_data, "two");

        // A custom name survives future upserts.
        store.accounts[0].display_name = "MyName".into();
        store.accounts[0].display_name_is_custom = true;
        store.upsert_session("abc", "three".into(), Some("LogName".into()));
        assert_eq!(store.get("abc").unwrap().display_name, "MyName");
    }

    #[test]
    fn upsert_clears_stale() {
        let (_dir, mut store) = temp_store();
        store.upsert_session("abc", "one".into(), None);
        store.mark_stale("abc");
        assert!(store.get("abc").unwrap().stale);
        store.upsert_session("abc", "fresh".into(), None);
        assert!(!store.get("abc").unwrap().stale);
    }

    #[test]
    fn refresh_log_name_upgrades_only_non_custom() {
        let (_dir, mut store) = temp_store();
        store.upsert_session("abc", "one".into(), None);
        assert_eq!(store.get("abc").unwrap().display_name, "Account abc");

        // A `None` name never clears the existing name.
        store.refresh_log_name("abc", None);
        assert_eq!(store.get("abc").unwrap().display_name, "Account abc");

        // A found name upgrades a generated one (case-insensitive id match)
        // without touching the token or `saved_at`.
        let saved_at = store.get("abc").unwrap().saved_at;
        store.refresh_log_name("ABC", Some("RealName".into()));
        assert_eq!(store.get("abc").unwrap().display_name, "RealName");
        assert_eq!(store.get("abc").unwrap().remember_me_data, "one");
        assert_eq!(store.get("abc").unwrap().saved_at, saved_at);

        // A user rename is never overwritten.
        store.accounts[0].display_name = "MyName".into();
        store.accounts[0].display_name_is_custom = true;
        store.refresh_log_name("abc", Some("LogName".into()));
        assert_eq!(store.get("abc").unwrap().display_name, "MyName");

        // An unknown account is a silent no-op.
        store.refresh_log_name("zzz", Some("X".into()));
    }

    #[test]
    fn remove_deletes_by_id() {
        let (_dir, mut store) = temp_store();
        store.upsert_session("abc", "one".into(), None);
        store.upsert_session("def", "two".into(), None);
        assert!(store.remove("ABC"));
        assert!(!store.remove("abc"));
        assert_eq!(store.accounts().len(), 1);
    }

    #[test]
    fn remove_tombstones_until_resaved() {
        let (dir, mut store) = temp_store();
        store.upsert_session("abc", "one".into(), None);
        store.remove("ABC");
        assert!(store.is_removed("abc"), "tombstone is case-insensitive");
        store.save().expect("save");

        // The tombstone survives a reload.
        let mut reloaded = AccountStore::load_from(dir.path().join(STORE_FILE));
        assert!(reloaded.is_removed("Abc"));

        // An explicit re-save lifts it.
        reloaded.upsert_session("abc", "fresh".into(), None);
        assert!(!reloaded.is_removed("abc"));
    }

    #[test]
    fn corrupt_store_is_quarantined() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE);
        std::fs::write(&path, b"{ not json").expect("write");
        let store = AccountStore::load_from(path.clone());
        assert!(store.accounts().is_empty());
        assert!(!path.exists());
        assert!(dir.path().join("accounts.json.bad").exists());
    }

    #[test]
    fn second_corruption_keeps_earlier_quarantine() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE);
        std::fs::write(&path, b"{ first corruption").expect("write");
        let _ = AccountStore::load_from(path.clone());
        std::fs::write(&path, b"{ second corruption").expect("write");
        let _ = AccountStore::load_from(path.clone());

        let first = std::fs::read_to_string(dir.path().join("accounts.json.bad")).expect("bad");
        let second =
            std::fs::read_to_string(dir.path().join("accounts.json.bad.1")).expect("bad.1");
        assert_eq!(first, "{ first corruption", "earlier quarantine must survive");
        assert_eq!(second, "{ second corruption");
    }

    #[test]
    fn plaintext_values_pass_through_unwrap() {
        assert_eq!(unwrap_data("rawblob").expect("plain"), "rawblob");
    }

    #[test]
    fn dpapi_roundtrip() {
        let wrapped = wrap_data("secret token").expect("wrap");
        assert!(wrapped.starts_with(DPAPI_PREFIX));
        assert_eq!(unwrap_data(&wrapped).expect("unwrap"), "secret token");
    }

    #[test]
    fn debug_redacts_the_blob() {
        let account = EpicAccount {
            account_id: "abc".into(),
            display_name: "Benny".into(),
            display_name_is_custom: false,
            remember_me_data: "SUPERSECRET".into(),
            saved_at: 0,
            last_used: None,
            stale: false,
            undecryptable_raw: None,
        };
        let debug = format!("{account:?}");
        assert!(!debug.contains("SUPERSECRET"));
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn undecryptable_blob_survives_an_unrelated_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE);
        // A store on disk with a blob that will fail to DPAPI-decrypt (valid
        // base64, wrong ciphertext) plus a normal plaintext-passthrough entry.
        let file = format!(
            r#"{{"version":1,"accounts":[
                {{"accountId":"bad","displayName":"Bad","rememberMeData":"dpapi:{}","savedAt":1}},
                {{"accountId":"good","displayName":"Good","rememberMeData":"plainblob","savedAt":2}}
            ]}}"#,
            base64::engine::general_purpose::STANDARD.encode(b"not a real dpapi blob")
        );
        std::fs::write(&path, file).expect("write");

        // Load blanks the bad token in memory but keeps the raw ciphertext.
        let mut store = AccountStore::load_from(path.clone());
        assert_eq!(store.get("bad").unwrap().remember_me_data, "");
        assert!(store.get("bad").unwrap().stale);

        // An unrelated action (touch the good account) triggers a full save.
        store.touch_last_used("good");
        store.save().expect("save");

        // The bad account's original ciphertext survived on disk byte-for-byte.
        let reloaded_raw = std::fs::read_to_string(&path).expect("read");
        let expected = base64::engine::general_purpose::STANDARD.encode(b"not a real dpapi blob");
        assert!(
            reloaded_raw.contains(&format!("dpapi:{expected}")),
            "undecryptable ciphertext must be preserved verbatim"
        );
    }
}
