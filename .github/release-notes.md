### Account switching
- Switch between saved Epic Games accounts from the Windows system tray without ever retyping a password
- One click closes the Epic Games Launcher, restores that account's saved login and reopens it already signed in, with its window visible
- Already-running games keep running, and helper processes left over from a crashed launcher get cleaned up so the next start does not hang
- Every switch re-saves the outgoing account's session, so its stored login stays fresh
- The account you are already on is marked active and greyed out, and picking it anyway is a harmless no-op
- Only one switch runs at a time — clicking another account mid-switch tells you to wait instead of starting a second launcher restart
- Quit and automatic updates both wait for a running switch to finish, so it can never be cut off half-way

### Saving and managing accounts
- "Save current account" snapshots whichever account is signed in to the launcher, while the launcher stays open
- New logins made directly in the Epic launcher are detected within seconds and saved automatically (on by default, switchable off)
- Accounts you remove stay removed — nothing re-adds them until you deliberately save that account again
- "Remove account" deletes only this app's local snapshot after a Yes/No confirmation that defaults to No; your Epic account is untouched
- Account names come from the Epic launcher's own logs, with a readable placeholder until a game has been launched once
- Accounts are listed with the active one first, then by most recent use

### Tray interface
- Runs entirely from the system tray — no app window ever opens, just the native menu and small system dialogs
- Left-click or right-click the tray icon to open the account menu
- The tray icon shows the active account's colored initial badge and its name as tooltip, falling back to the plain app icon when nobody is signed in
- Every account gets a generated, colour-coded initials badge so accounts are recognisable at a glance, including accented names
- The menu, icon and tooltip refresh on their own within a few seconds after any login, logout or switch made in the launcher
- Starting the app twice shows an "already running" notice instead of a second tray icon

### Settings
- All settings live in a tray submenu: language, display name, start with Windows, and automatic account saving
- Full English and German interface, defaulting to your Windows language and switchable instantly
- Show accounts by Epic username or, for privacy, by a shortened account ID — badges and tooltip follow the same choice
- Starts with Windows out of the box, toggleable from the tray, and your choice is remembered
- All settings persist across restarts

### Reliability and security
- Saved session tokens are encrypted with Windows DPAPI, so the account file is unusable to other Windows users or on another PC
- No administrator rights required — only per-user files and settings are touched
- Sessions Epic rejects are flagged "(expired)" with a dialog explaining how to log in again and re-save
- Every failed switch, save or setting change shows a clear dialog naming the actual cause instead of failing silently
- Only the login section of the launcher's config is rewritten, atomically and in its original format, so nothing else is disturbed
- Updates are checked at every start and installed automatically after signature verification

Download the installer below. The app updates itself automatically afterwards.
