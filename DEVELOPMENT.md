# Epic Quick Switch — Development

A Windows system-tray app to switch between Epic Games accounts. Built with
**Tauri v2** (Rust) plus a minimal React/TypeScript shell. The app is **tray-only** —
there is no application window; the entire UI is a native tray menu built in Rust.

## How account switching works

Unlike Steam, Epic's launcher keeps only **one** login session at a time: the
`[RememberMe]` token in
`%LocalAppData%\EpicGamesLauncher\Saved\Config\WindowsEditor\GameUserSettings.ini`
(some builds use `...\Config\Windows\`), plus the account identity in
`HKCU\Software\Epic Games\Unreal Engine\Identifiers\AccountId`.

The app therefore keeps its own store of session snapshots. A switch:

1. kills `EpicGamesLauncher.exe` **first** (it rewrites the ini on exit) plus its
   helper processes (orphaned helpers stall the next start),
2. snapshots the outgoing session into the store,
3. writes the target account's token into the ini (encoding-preserving,
   atomic) and its `AccountId` into the registry,
4. relaunches the launcher with `-silent`.

**No admin rights required** — everything is `HKCU` and per-user files.

Constraints:

- Windows only.
- Accounts must be logged in once with **"Remember me"** before they can be saved.
- Switching closes and reopens the launcher.
- Session tokens can expire server-side; the app detects this after a switch,
  marks the account, and asks the user to re-login + re-save.

## Project layout

- `src-tauri/src/` — Rust backend (the whole app):
  - `epic/` — Epic integration:
    - `ini.rs` — encoding-preserving `[RememberMe]` editor (UTF-8/UTF-16 LE)
    - `paths.rs` — session-ini/log/launcher-exe discovery
    - `registry.rs` — `AccountId` + protocol-handler lookup (`HKCU` writes only)
    - `logs.rs` — username extraction from launcher logs
    - `store.rs` — `accounts.json` snapshot store (DPAPI-wrapped tokens)
    - `accounts.rs` — UI account model + live-session probing
    - `switch.rs` — kill/snapshot/write/relaunch orchestration
    - `icon.rs` — generated initials badges (tray + menu icons)
  - `tray.rs` — native tray menu (accounts, save, remove, settings), watcher
  - `settings.rs` — `settings.json` via the store plugin (`language`, `nameMode`)
  - `i18n.rs` — English/German menu labels
  - `lib.rs` — app entry, plugin registration, background update check
- `src/` — minimal React shell; no window is shown, it only satisfies the build
- `.github/workflows/release.yml` — signed release build (tauri-action)

## Develop

```bash
npm install
npm run tauri dev     # runs the app (tray only — no window appears)
npm run tauri build   # builds the installer
cargo test --manifest-path src-tauri/Cargo.toml
```

Machine-dependent smoke tests (need an Epic install, print real values):

```bash
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
```

## Settings & data

- `%AppData%\Roaming\epic-quick-switch\settings.json` — `language`, `nameMode`
- `%AppData%\Roaming\epic-quick-switch\accounts.json` — account snapshots; the
  session tokens inside are DPAPI-encrypted (current-user scope) and are never
  logged. A corrupt store is quarantined as `accounts.json.bad`.

## Releasing

See [RELEASING.md](RELEASING.md).

## License

[MIT](LICENSE)
