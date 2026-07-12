# Releasing

Releases are built, signed, and published automatically by GitHub Actions
(`.github/workflows/release.yml`) whenever a `v*` tag is pushed.

## One-time setup

Add two repository secrets under **Settings → Secrets and variables → Actions**:

- `TAURI_SIGNING_PRIVATE_KEY` — the full contents of the local, git-ignored
  `src-tauri/eqs-updater.key` file. Get it with:

  ```powershell
  Get-Content src-tauri/eqs-updater.key -Raw
  ```

- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the key password, stored locally in the
  git-ignored `src-tauri/eqs-updater.key.txt`.

> Keep `eqs-updater.key` and its password secret and backed up (offline). If
> they are lost, already-installed apps can no longer verify and receive signed
> updates.

## Signing key security (important)

The updater public key is **baked into every released binary and cannot be
revoked**. This makes the private signing key the single trust anchor for all
auto-updates — treat it like a production credential:

- **A leak is unrecoverable for existing installs.** Anyone with the private key
  and its password can sign updates that already-installed clients will accept.
  The only remedy is shipping a new build with a *new* keypair, which existing
  users must install manually. There is no online revocation.
- **The key is password-protected**, so the key file alone is not immediately
  usable if it leaks. Store the password separately from the key wherever
  possible.
- **Keep both out of the repo.** `*.key` / `*.key.txt` are git-ignored; never
  commit them. Store them only in GitHub Actions secrets plus an offline backup,
  and minimise who can read the Actions secrets.
- **Restrict who can release.** Only trusted maintainers should be able to push
  `v*` tags. Consider putting the release job behind a GitHub Environment with
  required reviewers so a release cannot be cut (and the key cannot be used)
  without approval.

## Cutting a release

Just push a version tag — the workflow derives the app version from the tag, so
you do **not** need to edit any version files:

```
git tag v0.1.0
git push origin v0.1.0
```

The **Release** workflow writes the version (from the tag) into
`tauri.conf.json`, `package.json` and `Cargo.toml`, builds the Windows installer
(NSIS + MSI), signs the update artifacts, and publishes a GitHub Release with the
installers and `latest.json`. A follow-up job also uploads the NSIS installer
under the stable name `EpicQuickSwitch-Setup.exe` and updates the README
download badge.

> The tag (e.g. `v0.1.1`) must be a **higher** version than what users have
> installed, otherwise the updater sees no newer version and does nothing.

Installed apps fetch `latest.json` on startup and update themselves when a newer
version is available.
