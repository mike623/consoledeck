//! Key combos and typing. Combos are stored as readable text like "space" or "shift+cmd+t"
//! (modifiers ctrl, opt, shift, cmd = Ctrl, Alt, Shift, Win on Windows). Names map to physical
//! keys per OS; keys not listed are stored as "kc<native code>".

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Mac virtual key codes (physical positions, ANSI/ISO), same table as the Swift app.
#[cfg(target_os = "macos")]
const CODES: &[(&str, u32)] = &[
    ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5), ("z", 6), ("x", 7), ("c", 8), ("v", 9),
    ("§", 10), ("b", 11), ("q", 12), ("w", 13), ("e", 14), ("r", 15), ("y", 16), ("t", 17), ("1", 18),
    ("2", 19), ("3", 20), ("4", 21), ("6", 22), ("5", 23), ("=", 24), ("9", 25), ("7", 26), ("-", 27),
    ("8", 28), ("0", 29), ("]", 30), ("o", 31), ("u", 32), ("[", 33), ("i", 34), ("p", 35),
    ("return", 36), ("l", 37), ("j", 38), ("'", 39), ("k", 40), (";", 41), ("\\", 42), (",", 43),
    ("/", 44), ("n", 45), ("m", 46), (".", 47), ("tab", 48), ("space", 49), ("`", 50), ("delete", 51),
    ("escape", 53), ("f1", 122), ("f2", 120), ("f3", 99), ("f4", 118), ("f5", 96), ("f6", 97),
    ("f7", 98), ("f8", 100), ("f9", 101), ("f10", 109), ("f11", 103), ("f12", 111), ("home", 115),
    ("pageup", 116), ("forwarddelete", 117), ("end", 119), ("pagedown", 121), ("left", 123),
    ("right", 124), ("down", 125), ("up", 126),
];

/// Windows virtual-key codes (US layout positions for punctuation).
#[cfg(not(target_os = "macos"))]
const CODES: &[(&str, u32)] = &[
    ("a", 0x41), ("b", 0x42), ("c", 0x43), ("d", 0x44), ("e", 0x45), ("f", 0x46), ("g", 0x47),
    ("h", 0x48), ("i", 0x49), ("j", 0x4A), ("k", 0x4B), ("l", 0x4C), ("m", 0x4D), ("n", 0x4E),
    ("o", 0x4F), ("p", 0x50), ("q", 0x51), ("r", 0x52), ("s", 0x53), ("t", 0x54), ("u", 0x55),
    ("v", 0x56), ("w", 0x57), ("x", 0x58), ("y", 0x59), ("z", 0x5A), ("0", 0x30), ("1", 0x31),
    ("2", 0x32), ("3", 0x33), ("4", 0x34), ("5", 0x35), ("6", 0x36), ("7", 0x37), ("8", 0x38),
    ("9", 0x39), (";", 0xBA), ("=", 0xBB), (",", 0xBC), ("-", 0xBD), (".", 0xBE), ("/", 0xBF),
    ("`", 0xC0), ("[", 0xDB), ("\\", 0xDC), ("]", 0xDD), ("'", 0xDE), ("§", 0xE2),
    ("return", 0x0D), ("tab", 0x09), ("space", 0x20), ("delete", 0x08), ("escape", 0x1B),
    ("f1", 0x70), ("f2", 0x71), ("f3", 0x72), ("f4", 0x73), ("f5", 0x74), ("f6", 0x75), ("f7", 0x76),
    ("f8", 0x77), ("f9", 0x78), ("f10", 0x79), ("f11", 0x7A), ("f12", 0x7B), ("home", 0x24),
    ("pageup", 0x21), ("forwarddelete", 0x2E), ("end", 0x23), ("pagedown", 0x22), ("left", 0x25),
    ("up", 0x26), ("right", 0x27), ("down", 0x28),
];

const MODIFIERS: [(&str, Key); 4] =
    [("ctrl", Key::Control), ("opt", Key::Alt), ("shift", Key::Shift), ("cmd", Key::Meta)];

#[derive(Debug, PartialEq)]
pub struct KeyCombo {
    pub mods: Vec<Key>,
    pub code: u32,
}

impl KeyCombo {
    /// "cmd+shift+t", "space", "kc105" -> KeyCombo. None if any part is unknown.
    pub fn parse(text: &str) -> Option<KeyCombo> {
        let lower = text.to_lowercase();
        let mut parts: Vec<&str> = lower.split('+').map(str::trim).collect();
        let last = parts.pop().filter(|k| !k.is_empty())?;
        let code = match CODES.iter().find(|(n, _)| *n == last) {
            Some(&(_, c)) => c,
            None => last.strip_prefix("kc")?.parse().ok()?,
        };
        let mods = parts.iter().map(|p| MODIFIERS.iter().find(|(n, _)| n == p).map(|(_, k)| *k)).collect::<Option<_>>()?;
        Some(KeyCombo { mods, code })
    }
}

/// Posts the combo to whatever app is in front. Needs Accessibility permission on Mac.
pub fn press(combo: &KeyCombo) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let r = (|| {
        for m in &combo.mods {
            enigo.key(*m, Direction::Press)?;
        }
        enigo.key(Key::Other(combo.code), Direction::Click)?;
        for m in combo.mods.iter().rev() {
            enigo.key(*m, Direction::Release)?;
        }
        Ok::<_, enigo::InputError>(())
    })();
    r.map_err(|e| e.to_string())
}

/// Types text into the app in front. Newlines are real Return presses, since many apps ignore a typed "\n".
pub fn type_text(text: &str) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let ret = KeyCombo::parse("return").unwrap().code;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            enigo.key(Key::Other(ret), Direction::Click).map_err(|e| e.to_string())?;
        }
        if !line.is_empty() {
            enigo.text(line).map_err(|e| e.to_string())?;
        }
        std::thread::sleep(std::time::Duration::from_millis(5)); // some apps drop events that arrive in one burst
    }
    Ok(())
}

/// Mac: with prompt, macOS shows its "allow ConsoleDeck to control this computer" dialog if not yet trusted.
/// Windows needs no permission.
#[cfg(target_os = "macos")]
pub fn accessibility_trusted(prompt: bool) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
    }
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt"),
        if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() },
    )]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_trusted(_prompt: bool) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        assert_eq!(KeyCombo::parse("space"), KeyCombo::parse("SPACE"));
        let t = KeyCombo::parse("shift+cmd+t").unwrap();
        assert_eq!(t.mods, [Key::Shift, Key::Meta]);
        assert_eq!(t.code, CODES.iter().find(|(n, _)| *n == "t").unwrap().1);
        assert_eq!(KeyCombo::parse("opt+return").unwrap().mods, [Key::Alt]);
        assert_eq!(KeyCombo::parse("kc105").unwrap().code, 105);
        assert_eq!(KeyCombo::parse("hyper+t"), None);
        assert_eq!(KeyCombo::parse(""), None);
        assert_eq!(KeyCombo::parse("ctrl+"), None);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_codes_match_swift_app() {
        assert_eq!(KeyCombo::parse("space").unwrap().code, 49);
        assert_eq!(KeyCombo::parse("cmd+shift+t").unwrap().code, 17);
        assert_eq!(KeyCombo::parse("ctrl+cmd+q").unwrap().code, 12);
    }

    #[test]
    fn every_name_is_unique() {
        let mut names: Vec<_> = CODES.iter().map(|(n, _)| n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), CODES.len());
    }
}
