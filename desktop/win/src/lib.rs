//! SonicScript for Windows as a library (M2 plan task 2.1): the M1 dictation app's modules, linked by the CLI
//! (`src/main.rs`, `sonicscript.exe`) and by the tray app (`desktop/app`, `SonicScript.exe`).

#[cfg(windows)]
pub mod audio;
pub mod bench;
#[cfg(windows)]
pub mod cue;
#[cfg(windows)]
pub mod dictate;
pub mod events;
#[cfg(windows)]
pub mod history;
#[cfg(windows)]
pub mod hotkey;
#[cfg(windows)]
pub mod inject;
pub mod key;
pub mod keystore;
pub mod models;
#[cfg(windows)]
pub mod mywords;
pub mod setkey;
pub mod settings;
pub mod wav;
