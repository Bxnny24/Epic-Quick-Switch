<div align="center">

# Epic Quick Switch

*Switch between your Epic Games accounts straight from the system tray — one click, no retyping passwords.*

<!-- BEGIN LATEST DOWNLOAD BUTTON -->
[![Download](https://img.shields.io/badge/Download-Epic_Quick_Switch_0.1.0-7c3aed?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/Bxnny24/Epic-Quick-Switch/releases/latest/download/EpicQuickSwitch-Setup.exe)

![Platform](https://img.shields.io/badge/platform-Windows_10%2F11-lightgrey?style=flat-square&logo=windows)
![Version](https://img.shields.io/badge/version-0.1.0-7c3aed?style=flat-square)
<!-- END LATEST DOWNLOAD BUTTON -->

</div>

---

## What is Epic Quick Switch?

Epic Quick Switch is a lightweight Windows tray app for anyone juggling more than
one Epic Games account. It lives **entirely in the system tray** — there is no
window to open.

- **Right- or left-click the tray icon** to see your saved Epic accounts
- **Click an account to switch** — the Epic Games Launcher restarts and logs
  straight into it, no password retyping
- The **tray icon shows the active account's** colored initials badge
- **Settings right in the tray menu:** language (English/German), display name
  (Epic name or account ID), and start with Windows
- **Updates itself automatically**

---

## How to use

1. **Download and run the installer** (button above). Windows SmartScreen may warn
   because the app isn't code-signed yet — click **More info → Run anyway**.
2. A new icon appears in your **system tray** (bottom-right; you may need the `^`
   arrow to reveal hidden icons).
3. **Add your first account:** log in to the Epic Games Launcher with
   **"Remember me" ticked**, then click **"Save current account"** in the tray menu.
4. **Repeat for each account:** log out in the launcher, log in with the next
   account (Remember me on), and save it too.
5. From now on, **click an account in the tray menu to switch**. Done.

> Tip: drag the tray icon out of the `^` overflow so it's always visible.

---

## Good to know

- **Windows only.**
- Epic's launcher remembers only **one login at a time** — that's why this app
  keeps its own snapshot of each account's session (see below) and why every
  account must be logged in once with **"Remember me"** before it can be saved.
- Switching **closes and reopens the Epic Games Launcher** (running games keep
  running).
- Epic **sessions expire** after a while or when Epic invalidates them. If a
  switch lands on the login screen, just log in once more (Remember me on) and
  click **"Save current account"** again — the entry is refreshed. The app also
  re-snapshots the account you're switching *away from* on every switch, which
  keeps tokens fresh automatically.
- Account **names** come from the launcher's logs and are only available after a
  game was started once; until then the account shows as `Account <id>`.
- **Security:** the saved session tokens are stored in
  `%AppData%\epic-quick-switch\accounts.json`, encrypted with Windows DPAPI so
  they are only readable by your Windows user on this machine. (Epic's own
  launcher stores the very same token unencrypted in your profile.)

---

<div align="center">

Developer? See **[DEVELOPMENT.md](DEVELOPMENT.md)** · [MIT License](LICENSE)

</div>
