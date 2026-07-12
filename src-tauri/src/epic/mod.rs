//! Epic Games Launcher integration.
//!
//! Unlike Steam, the Epic launcher keeps only ONE login session at a time
//! (the `[RememberMe]` token in `GameUserSettings.ini`). Switching therefore
//! means: snapshot the current session into this app's own store, write the
//! target account's snapshot back, and restart the launcher.

pub mod accounts;
pub mod icon;
pub mod ini;
pub mod logs;
pub mod paths;
pub mod registry;
pub mod store;
pub mod switch;

pub use accounts::{list_accounts, Account};
