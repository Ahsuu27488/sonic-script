//! The dictation key is the user's choice (M1 plan step 6, D-018, D-037): which keys are allowed, how each one
//! behaves, the setting in `%APPDATA%\SonicScript\config.toml` (`hotkey = "RightCtrl"`) and the `--set-key`
//! command ("press the key you want", or a name).

use std::path::PathBuf;

/// What triggers dictation: a keyboard key (virtual-key code, left / right specific) or a mouse side button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Key(u16),
    /// `XBUTTON1` (1, usually "Back") or `XBUTTON2` (2, usually "Forward").
    Mouse(u16),
}

/// How a key reaches the app in front (D-037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassOn {
    /// Ctrl, Shift: harmless alone, and their shortcuts keep working.
    Always,
    /// Alt, Win: passed on for their shortcuts, but a tap alone would open the menu bar / Start menu, so a
    /// harmless keystroke (an unassigned key code) is sent while the key is down.
    Masked,
    /// Keys that do something alone (F-keys, Insert, Caps Lock, Pause, Scroll Lock) and mouse side buttons
    /// (Back / Forward in browsers): kept from the app.
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictationKey {
    /// The name in `config.toml` and `--set-key <name>`.
    pub name: &'static str,
    /// What the console calls it ("Right Ctrl").
    pub label: &'static str,
    pub trigger: Trigger,
    pub pass_on: PassOn,
    /// Shown when it is chosen: what the user gives up.
    pub warning: Option<&'static str>,
}

/// The default (D-015).
pub const DEFAULT: &str = "RightCtrl";

/// An unassigned virtual-key code (0xE8), sent to keep a lone Alt / Win tap from opening a menu.
pub const MASK_VK: u16 = 0xE8;

const SHIFT_WARNING: &str =
    "every capital letter you type with this Shift will start and cancel a recording";

macro_rules! key {
    ($name:literal, $label:literal, $trigger:expr, $pass:ident, $warning:expr) => {
        DictationKey {
            name: $name,
            label: $label,
            trigger: $trigger,
            pass_on: PassOn::$pass,
            warning: $warning,
        }
    };
}

const FIXED: &[DictationKey] = &[
    key!("RightCtrl", "Right Ctrl", Trigger::Key(0xA3), Always, None),
    key!(
        "LeftCtrl",
        "Left Ctrl",
        Trigger::Key(0xA2),
        Always,
        Some(
            "Ctrl shortcuts with the left Ctrl (Ctrl+C, Ctrl+V, ...) will each start and cancel a recording"
        )
    ),
    key!("RightAlt", "Right Alt", Trigger::Key(0xA5), Masked, None),
    key!(
        "LeftAlt",
        "Left Alt",
        Trigger::Key(0xA4),
        Masked,
        Some(
            "Alt shortcuts with the left Alt (Alt+Tab, ...) will each start and cancel a recording"
        )
    ),
    key!(
        "RightShift",
        "Right Shift",
        Trigger::Key(0xA1),
        Always,
        Some(SHIFT_WARNING)
    ),
    key!(
        "LeftShift",
        "Left Shift",
        Trigger::Key(0xA0),
        Always,
        Some(SHIFT_WARNING)
    ),
    key!(
        "RightWin",
        "Right Windows key",
        Trigger::Key(0x5C),
        Masked,
        None
    ),
    key!("Pause", "Pause", Trigger::Key(0x13), Never, None),
    key!("ScrollLock", "Scroll Lock", Trigger::Key(0x91), Never, None),
    key!(
        "CapsLock",
        "Caps Lock",
        Trigger::Key(0x14),
        Never,
        Some("Caps Lock will no longer switch capitals on and off")
    ),
    key!("Insert", "Insert", Trigger::Key(0x2D), Never, None),
    key!(
        "Mouse4",
        "the mouse Back side button",
        Trigger::Mouse(1),
        Never,
        Some("the button will no longer go back in browsers and folders")
    ),
    key!(
        "Mouse5",
        "the mouse Forward side button",
        Trigger::Mouse(2),
        Never,
        Some("the button will no longer go forward in browsers and folders")
    ),
];

const F_NAMES: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

/// Every allowed key: the fixed ones, then F1-F24 (virtual keys 0x70-0x87).
pub fn allowed() -> impl Iterator<Item = DictationKey> {
    FIXED.iter().copied().chain(
        F_NAMES
            .iter()
            .zip(0x70u16..)
            .map(|(&name, vk)| DictationKey {
                name,
                label: name,
                trigger: Trigger::Key(vk),
                pass_on: PassOn::Never,
                warning: None,
            }),
    )
}

/// Comparable form of a name: lower case, no spaces, dashes or underscores ("Right Ctrl" = "right-ctrl").
fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A key by its name; also takes common spellings ("Right Control", "RCtrl", "XButton1", "Mouse Back").
pub fn by_name(name: &str) -> Result<DictationKey, String> {
    let wanted = fold(name);
    let alias = match wanted.as_str() {
        "rctrl" | "rightcontrol" | "rcontrol" => "RightCtrl",
        "lctrl" | "leftcontrol" | "lcontrol" => "LeftCtrl",
        "ralt" | "altgr" => "RightAlt",
        "lalt" => "LeftAlt",
        "rshift" => "RightShift",
        "lshift" => "LeftShift",
        "rwin" | "rightwindows" | "rightwindowskey" => "RightWin",
        "scroll" | "scrlk" => "ScrollLock",
        "caps" => "CapsLock",
        "ins" => "Insert",
        "break" => "Pause",
        "xbutton1" | "mouseback" | "back" => "Mouse4",
        "xbutton2" | "mouseforward" | "forward" => "Mouse5",
        _ => "",
    };
    allowed()
        .find(|k| fold(k.name) == wanted || k.name == alias)
        .ok_or_else(|| match refused(&wanted) {
            Some(why) => format!("\"{name}\" cannot be the dictation key: {why}."),
            None => format!(
                "\"{name}\" is not a key SonicScript knows. Allowed: {}.",
                allowed().map(|k| k.name).collect::<Vec<_>>().join(", ")
            ),
        })
}

/// The reason a known key name is refused, if it is one.
fn refused(folded: &str) -> Option<&'static str> {
    Some(match folded {
        "enter" | "return" => "Enter stops a recording",
        "esc" | "escape" => "Esc cancels a recording",
        "space" | "tab" | "backspace" | "delete" | "del" => "it types or edits text",
        "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown" => {
            "it moves the cursor"
        }
        "leftwin" | "lwin" | "win" | "windows" => {
            "the left Windows key opens Start and runs Windows shortcuts"
        }
        "ctrl" | "control" | "alt" | "shift" => "say which side (for example RightCtrl or LeftAlt)",
        s if s.chars().count() == 1 => "it types a character",
        _ => return None,
    })
}

/// The key pressed during `--set-key`, or why it cannot be used.
pub fn by_trigger(trigger: Trigger) -> Result<DictationKey, String> {
    if let Some(k) = allowed().find(|k| k.trigger == trigger) {
        return Ok(k);
    }
    let Trigger::Key(vk) = trigger else {
        return Err("only the two side buttons of a mouse can be used".into());
    };
    Err(match vk {
        0x0D => "Enter stops a recording".into(),
        0x1B => "Esc cancels a recording".into(),
        0x5B => "the left Windows key opens Start and runs Windows shortcuts".into(),
        0x08 | 0x09 | 0x20 | 0x2E => "it types or edits text".into(),
        0x21..=0x28 => "it moves the cursor".into(),
        0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xE2 => "it types a character".into(),
        0x90 => "Num Lock changes what the number keys do".into(),
        0xA6..=0xB7 => "it is a media or browser key".into(),
        _ => format!("this key (code {vk:#04x}) is not one of the allowed keys"),
    })
}

/// The key from the settings (D-045); Right Ctrl when there is none. A bad value gives Right Ctrl and a notice.
pub fn load() -> (DictationKey, Option<String>) {
    let (settings, _) = crate::settings::load();
    from_settings(&settings)
}

/// The key a `Settings` names; Right Ctrl and a notice when it is not an allowed key.
pub fn from_settings(settings: &crate::settings::Settings) -> (DictationKey, Option<String>) {
    match by_name(&settings.hotkey) {
        Ok(k) => (k, None),
        Err(e) => (
            by_name(DEFAULT).expect("the default key is allowed"),
            Some(format!("config.toml (hotkey): {e} Using Right Ctrl.")),
        ),
    }
}

/// Saves the key in the settings, keeping the others, and returns the file's path.
pub fn save(key: &DictationKey) -> Result<PathBuf, String> {
    let path = crate::settings::path()?;
    let (mut settings, _) = crate::settings::Settings::load(&path);
    settings.hotkey = key.name.to_string();
    settings
        .save(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// `--set-key [name | list]`: `None` when the arguments are not this command.
pub fn command(args: &[String]) -> Option<Result<(), String>> {
    let (first, rest) = args.split_first()?;
    if first != "--set-key" {
        return None;
    }
    Some(match rest {
        [] => interactive(),
        [list] if list == "list" => {
            print_list();
            Ok(())
        }
        names => by_name(&names.join(" ")).and_then(|k| choose(&k)),
    })
}

fn print_list() {
    println!("Keys you can use for dictation (sonicscript --set-key <name>):");
    for k in allowed() {
        let how = match k.pass_on {
            PassOn::Always | PassOn::Masked => "still works for shortcuts",
            PassOn::Never => "only starts dictation",
        };
        let warn = k.warning.map(|w| format!("; {w}")).unwrap_or_default();
        println!("  {:<11} {} ({how}{warn})", k.name, k.label);
    }
}

fn choose(key: &DictationKey) -> Result<(), String> {
    let path = save(key)?;
    println!(
        "Dictation key: {} (saved in {}).",
        key.label,
        path.display()
    );
    if let Some(w) = key.warning {
        println!("  Note: {w}. sonicscript --set-key changes it again.");
    }
    if key.pass_on == PassOn::Never {
        println!("  It now only starts dictation: apps no longer see it.");
    }
    println!("  Restart SonicScript if it is running.");
    Ok(())
}

#[cfg(windows)]
fn interactive() -> Result<(), String> {
    let current = load().0;
    println!(
        "Press the key (or mouse side button) you want for dictation. Now: {}. Esc keeps it.",
        current.label
    );
    let presses = crate::hotkey::capture()?;
    loop {
        let trigger = presses
            .recv()
            .map_err(|_| "the key hook stopped".to_string())?;
        if trigger == Trigger::Key(0x1B) {
            println!("Kept {}.", current.label);
            return Ok(());
        }
        match by_trigger(trigger) {
            Ok(k) => return choose(&k),
            Err(why) => println!(
                "  That key cannot be used: {why}. Press another one (Esc keeps {}).",
                current.label
            ),
        }
    }
}

#[cfg(not(windows))]
fn interactive() -> Result<(), String> {
    Err("--set-key without a name needs Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_spellings_find_the_key() {
        assert_eq!(by_name("RightCtrl").unwrap().trigger, Trigger::Key(0xA3));
        assert_eq!(by_name("right ctrl").unwrap().name, "RightCtrl");
        assert_eq!(by_name("Right-Control").unwrap().name, "RightCtrl");
        assert_eq!(by_name("f9").unwrap().trigger, Trigger::Key(0x78));
        assert_eq!(by_name("F24").unwrap().trigger, Trigger::Key(0x87));
        assert_eq!(by_name("XButton1").unwrap().trigger, Trigger::Mouse(1));
        assert_eq!(by_name("mouse forward").unwrap().name, "Mouse5");
        assert_eq!(by_name("caps").unwrap().name, "CapsLock");
    }

    #[test]
    fn keys_that_type_stop_or_cancel_are_refused_with_a_reason() {
        for (name, why) in [
            ("Space", "types or edits"),
            ("Enter", "stops a recording"),
            ("Esc", "cancels"),
            ("a", "types a character"),
            ("7", "types a character"),
            ("Tab", "types or edits"),
            ("Up", "moves the cursor"),
            ("Ctrl", "which side"),
            ("LeftWin", "Start"),
        ] {
            let e = by_name(name).unwrap_err();
            assert!(e.contains(why), "{name}: {e}");
        }
        assert!(
            by_name("Banana")
                .unwrap_err()
                .contains("Allowed: RightCtrl")
        );
        for (vk, why) in [
            (0x20, "edits"),
            (0x0D, "stops"),
            (0x1B, "cancels"),
            (0x41, "character"),
            (0x25, "cursor"),
        ] {
            assert!(
                by_trigger(Trigger::Key(vk)).unwrap_err().contains(why),
                "{vk:#x}"
            );
        }
        assert!(by_trigger(Trigger::Mouse(3)).is_err());
    }

    #[test]
    fn a_pressed_key_is_found_by_its_code() {
        assert_eq!(by_trigger(Trigger::Key(0x78)).unwrap().name, "F9");
        assert_eq!(by_trigger(Trigger::Key(0xA5)).unwrap().name, "RightAlt");
        assert_eq!(by_trigger(Trigger::Mouse(2)).unwrap().name, "Mouse5");
    }

    #[test]
    fn keys_pass_on_as_d037_says() {
        let pass = |n| by_name(n).unwrap().pass_on;
        assert_eq!(pass("RightCtrl"), PassOn::Always);
        assert_eq!(pass("LeftShift"), PassOn::Always);
        assert_eq!(pass("RightAlt"), PassOn::Masked);
        assert_eq!(pass("RightWin"), PassOn::Masked);
        for n in ["F9", "Insert", "CapsLock", "Pause", "ScrollLock", "Mouse4"] {
            assert_eq!(pass(n), PassOn::Never, "{n}");
        }
        assert!(by_name("CapsLock").unwrap().warning.is_some());
        assert!(by_name("RightCtrl").unwrap().warning.is_none());
        let names: Vec<_> = allowed().map(|k| fold(k.name)).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "names are unique");
    }

    #[test]
    fn a_bad_key_in_the_settings_gives_right_ctrl_and_a_notice() {
        let mut s = crate::settings::Settings::default();
        assert_eq!(from_settings(&s).0.name, "RightCtrl");
        s.hotkey = "F9".into();
        assert_eq!(from_settings(&s), (by_name("F9").unwrap(), None));
        s.hotkey = "Space".into();
        let (k, note) = from_settings(&s);
        assert_eq!(k.name, "RightCtrl");
        assert!(note.unwrap().contains("Right Ctrl"));
    }
}
