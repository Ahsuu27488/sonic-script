//! Start with Windows (M2 task 2.3): the HKCU `Run` value `SonicScript` = the quoted path of this exe. Off
//! until the user ticks it in the tray menu; the installer (Phase 7) will turn it on, so a dev build never
//! registers a `target\` path by itself.

use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "SonicScript";

/// The value Windows runs at sign-in: the exe's path in quotes (it may contain spaces).
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Whether the `Run` value points at this exe.
pub fn enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN, KEY_READ)
        .and_then(|k| k.get_value::<String, _>(NAME))
        .is_ok_and(|v| v.eq_ignore_ascii_case(&command_line(&exe)))
}

/// Adds or removes the `Run` value.
pub fn set(on: bool) -> Result<(), String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN, KEY_READ | KEY_SET_VALUE)
        .map_err(|e| format!("could not open the Run key: {e}"))?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(NAME, &command_line(&exe))
            .map_err(|e| format!("could not turn on start with Windows: {e}"))
    } else {
        match key.delete_value(NAME) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("could not turn off start with Windows: {e}"))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_the_path() {
        assert_eq!(
            command_line(Path::new(r"C:\Program Files\SonicScript\SonicScript.exe")),
            r#""C:\Program Files\SonicScript\SonicScript.exe""#
        );
    }

    /// Writes and removes the real HKCU `Run` value (for this test exe): `cargo test -p sonicscript-app --
    /// --ignored`. A value that pointed at this exe before is put back.
    #[test]
    #[ignore]
    fn run_value_round_trip() {
        let before = enabled();
        set(true).unwrap();
        assert!(enabled());
        set(false).unwrap();
        assert!(!enabled());
        set(false).unwrap(); // already off: fine
        if before {
            set(true).unwrap();
        }
    }
}
