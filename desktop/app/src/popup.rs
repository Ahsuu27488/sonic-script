//! The pop-up mic's rules (M2 phase 5, D-016), pure so they are tested without UI Automation: which focused
//! element gets the pill, and where the pill goes. `focuswatch.rs` reads the focused element; `popupwin.rs`
//! shows the pill.

/// Debug builds: appends a line to the file named by `SONICSCRIPT_DEBUG_LOG` (checks of the focus watcher).
pub fn debug_log(line: &str) {
    if cfg!(debug_assertions)
        && let Some(path) = std::env::var_os("SONICSCRIPT_DEBUG_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

/// A screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// UI Automation control types that hold text the user types (`UIA_EditControlTypeId`,
/// `UIA_DocumentControlTypeId`, `UIA_ComboBoxControlTypeId`).
pub const EDIT: i32 = 50004;
pub const DOCUMENT: i32 = 50030;
pub const COMBO_BOX: i32 = 50003;

/// What UI Automation says about the element that just got the keyboard focus.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Focus {
    pub control_type: i32,
    pub enabled: bool,
    pub password: bool,
    /// `Some(read_only)` when the element has a Value pattern.
    pub value_read_only: Option<bool>,
    pub rect: Rect,
    pub pid: u32,
    /// The program in front (`notepad.exe`) and its window class.
    pub exe: String,
    pub class: String,
}

/// Text the user can type into: an edit box or document whose value is not read-only. A document without a
/// Value pattern (a web page's body, a PDF) is not; an edit box without one (rare) is.
pub fn editable(f: &Focus) -> bool {
    if !f.enabled || f.password {
        return false;
    }
    match (f.control_type, f.value_read_only) {
        (_, Some(true)) => false,
        (EDIT | DOCUMENT | COMBO_BOX, Some(false)) => true,
        (EDIT, None) => true,
        _ => false,
    }
}

/// Programs that host terminals, whatever their window's class (D-016: no pill there). A shell's own text
/// lives in one of these, never in `cmd.exe` / `pwsh.exe` (which may also show ordinary windows).
const TERMINALS: [&str; 3] = ["WindowsTerminal.exe", "OpenConsole.exe", "conhost.exe"];

/// Whether this focus gets the pill: the setting is on, the field is editable and not a password, it is not
/// in a terminal (off by default, D-016), not one of our own windows (the settings page runs in WebView2's
/// own process, so the program in front is checked too), and not in an app the user turned off.
pub fn shows(f: &Focus, popup_mic: bool, off_apps: &[String], own_pid: u32, own_exe: &str) -> bool {
    popup_mic
        && f.pid != own_pid
        && !f.exe.eq_ignore_ascii_case(own_exe)
        && editable(f)
        && !sonicscript::inject::is_terminal(&f.class)
        && !TERMINALS.iter().any(|t| t.eq_ignore_ascii_case(&f.exe))
        && !off_apps.iter().any(|a| a.eq_ignore_ascii_case(&f.exe))
        && f.rect.width() > 8
        && f.rect.height() > 8
}

/// Where the pill (a `size` square) goes: just below the field's right end; when there is no room below (a
/// field at the bottom of the screen, or an editor filling the window), inside its bottom-right corner. Always
/// kept on the monitor's work area.
pub fn place(field: Rect, work: Rect, size: i32, gap: i32) -> (i32, i32) {
    let below = field.bottom + gap;
    let y = if below + size <= work.bottom && field.height() < 160 {
        below
    } else {
        field.bottom.min(work.bottom) - size - gap
    };
    let x = field.right - size;
    (
        x.clamp(work.left, work.right - size),
        y.clamp(work.top, work.bottom - size),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notepad() -> Focus {
        Focus {
            control_type: EDIT,
            enabled: true,
            password: false,
            value_read_only: Some(false),
            rect: Rect {
                left: 100,
                top: 100,
                right: 500,
                bottom: 130,
            },
            pid: 42,
            exe: "notepad.exe".into(),
            class: "Notepad".into(),
        }
    }

    #[test]
    fn editable_fields_get_the_pill() {
        assert!(shows(&notepad(), true, &[], 1, "sonicscript-tray.exe"));
        let doc = Focus {
            control_type: DOCUMENT,
            ..notepad()
        };
        assert!(
            shows(&doc, true, &[], 1, "sonicscript-tray.exe"),
            "a contenteditable document"
        );
    }

    #[test]
    fn never_on_passwords_read_only_or_pages() {
        let f = |change: fn(&mut Focus)| {
            let mut x = notepad();
            change(&mut x);
            shows(&x, true, &[], 1, "sonicscript-tray.exe")
        };
        assert!(!f(|x| x.password = true));
        assert!(!f(|x| x.value_read_only = Some(true)));
        assert!(!f(|x| x.enabled = false));
        assert!(
            !f(|x| {
                x.control_type = DOCUMENT;
                x.value_read_only = None;
            }),
            "a web page's body"
        );
        assert!(!f(|x| x.control_type = 50000), "a button");
    }

    #[test]
    fn not_in_terminals_our_windows_turned_off_apps_or_when_off() {
        let term = Focus {
            class: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
            ..notepad()
        };
        assert!(!shows(&term, true, &[], 1, "sonicscript-tray.exe"));
        assert!(
            !shows(&notepad(), true, &[], 42, "sonicscript-tray.exe"),
            "our own window"
        );
        assert!(!shows(
            &notepad(),
            true,
            &["NOTEPAD.EXE".into()],
            1,
            "sonicscript-tray.exe"
        ));
        assert!(!shows(&notepad(), false, &[], 1, "sonicscript-tray.exe"));
        // The settings page: WebView2's process, our program in front.
        let ours = Focus {
            exe: "SonicScript-Tray.exe".into(),
            ..notepad()
        };
        assert!(!shows(&ours, true, &[], 1, "sonicscript-tray.exe"));
        // Windows Terminal, whatever class its window has.
        let wt = Focus {
            exe: "WindowsTerminal.exe".into(),
            ..notepad()
        };
        assert!(!shows(&wt, true, &[], 1, "sonicscript-tray.exe"));
        // A window a PowerShell script shows is an ordinary app.
        let form = Focus {
            exe: "pwsh.exe".into(),
            ..notepad()
        };
        assert!(shows(&form, true, &[], 1, "sonicscript-tray.exe"));
    }

    #[test]
    fn placed_below_right_or_inside_the_corner() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let field = notepad().rect;
        assert_eq!(place(field, work, 36, 6), (464, 136));
        // At the bottom of the screen: inside the corner.
        let low = Rect {
            top: 1000,
            bottom: 1030,
            ..field
        };
        assert_eq!(place(low, work, 36, 6), (464, 988));
        // An editor filling the window: inside its corner, on screen.
        let editor = Rect {
            left: 0,
            top: 80,
            right: 1920,
            bottom: 1200,
        };
        assert_eq!(place(editor, work, 36, 6), (1884, 998));
        // A field at the left edge partly off screen stays on it.
        let off = Rect {
            left: -50,
            top: 100,
            right: 10,
            bottom: 130,
        };
        assert_eq!(place(off, work, 36, 6).0, 0);
    }
}
