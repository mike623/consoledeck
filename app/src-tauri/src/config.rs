//! deck.json, shared with the Swift app and deck.py.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Physical 3-3-4 layout, numbered left-top to right-bottom (same as deck.py).
pub const POSITIONS: [u8; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl Action {
    #[cfg(test)]
    pub fn new(kind: &str, value: &str) -> Self {
        Action { kind: kind.into(), value: Some(value.into()) }
    }

    pub fn none() -> Self {
        Action { kind: "none".into(), value: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gesture {
    Press,
    Long,
    Double,
}

impl Gesture {
    /// Key in deck.json's buttons: "3", "3.long", "3.double". deck.py only knows plain "3".
    pub fn key(self, n: u8) -> String {
        match self {
            Gesture::Press => n.to_string(),
            Gesture::Long => format!("{n}.long"),
            Gesture::Double => format!("{n}.double"),
        }
    }
}

/// Per-app overrides. A button missing here falls back to the default buttons.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub buttons: BTreeMap<String, Action>,
    pub name: String,
}

/// Every key is optional so files written before a key existed still load.
/// Fields and maps are sorted so saves match the Swift app's sorted-keys output.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub buttons: BTreeMap<String, Action>, // the default profile; deck.py only knows this one
    // Gesture timing knobs; no UI, edit deck.json if presses feel too fast or slow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_press_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_press_ms: Option<u64>,
    pub pins: BTreeMap<String, u8>,
    pub profiles: BTreeMap<String, Profile>, // keyed by bundle ID (Mac) / exe name (Windows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_seconds: Option<u64>, // how often a "repeat" action presses its keys
}

impl Config {
    /// Same file as the Swift app on Mac (~/Library/Application Support), %APPDATA% on Windows.
    pub fn path() -> PathBuf {
        dirs::data_dir().expect("no user data folder").join("ConsoleDeck").join("deck.json")
    }

    /// A missing file is an empty config; a broken one is an error so the caller keeps the last good one.
    pub fn load() -> Result<Config, String> {
        match std::fs::read_to_string(Self::path()) {
            Ok(json) => Self::parse(&json),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn parse(json: &str) -> Result<Config, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// Written to a temp file and renamed, so deck.py or a crash never sees half a file.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())
    }

    /// None removes the button: back to "use default" in a profile, "none" in the default.
    /// A profile that no longer exists is left alone.
    pub fn set_action(&mut self, action: Option<Action>, n: u8, gesture: Gesture, profile: Option<&str>) {
        let buttons = match profile {
            None => &mut self.buttons,
            Some(p) => match self.profiles.get_mut(p) {
                Some(profile) => &mut profile.buttons,
                None => return,
            },
        };
        match action {
            Some(a) => buttons.insert(gesture.key(n), a),
            None => buttons.remove(&gesture.key(n)),
        };
    }

    /// Front app's profile first, then the default buttons. The name is None when the default was used.
    pub fn action(&self, n: u8, gesture: Gesture, app: Option<&str>) -> (Action, Option<String>) {
        if let Some(profile) = app.and_then(|a| self.profiles.get(a)) {
            if let Some(action) = profile.buttons.get(&gesture.key(n)) {
                return (action.clone(), Some(profile.name.clone()));
            }
        }
        (self.buttons.get(&gesture.key(n)).cloned().unwrap_or_else(Action::none), None)
    }
}

/// "example.com" opens as https://example.com, like the Swift app.
pub fn normalize_url(value: &str) -> String {
    if value.contains("://") { value.to_string() } else { format!("https://{value}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFARI: Option<&str> = Some("com.apple.Safari");

    fn profiled() -> Config {
        Config::parse(r#"{
            "pins": {"D6": 1, "D7": 2, "D8": 3},
            "buttons": {"1": {"type": "app", "value": "Spotify"}, "2": {"type": "app", "value": "Finder"},
                        "1.long": {"type": "app", "value": "Music"}},
            "profiles": {"com.apple.Safari": {"name": "Safari", "buttons": {
                "1": {"type": "url", "value": "https://x.com"}, "2": {"type": "none"}}}}
        }"#).unwrap()
    }

    #[test]
    fn reads_deck_py_config() {
        let c = Config::parse(r#"{"pins": {"D6": 1, "D5": 5}, "buttons": {"1": {"type": "app", "value": "Spotify"}}}"#).unwrap();
        assert_eq!(c.pins["D6"], 1);
        assert_eq!(c.action(1, Gesture::Press, None).0, Action::new("app", "Spotify"));
        assert_eq!(c.action(5, Gesture::Press, None).0, Action::none());
        assert!(c.profiles.is_empty());
    }

    #[test]
    fn reads_swift_app_config() {
        let c = Config::parse(r#"{
            "buttons": {"1": {"type": "url", "value": "x.com"}, "1.long": {"type": "none"}},
            "doublePressMs": 250, "longPressMs": 600, "pins": {"A2": 8},
            "profiles": {"com.apple.Safari": {"buttons": {"1": {"type": "none"}}, "name": "Safari"}},
            "repeatSeconds": 30
        }"#).unwrap();
        assert_eq!((c.long_press_ms, c.double_press_ms, c.repeat_seconds), (Some(600), Some(250), Some(30)));
        assert_eq!(c.profiles["com.apple.Safari"].name, "Safari");
        assert_eq!(c.pins["A2"], 8);
    }

    #[test]
    fn missing_keys_default_and_broken_json_errors() {
        assert_eq!(Config::parse("{}").unwrap(), Config::default());
        assert!(Config::parse("{\"pins\": ").is_err());
    }

    #[test]
    fn round_trips() {
        let c = profiled();
        assert_eq!(Config::parse(&serde_json::to_string_pretty(&c).unwrap()).unwrap(), c);
    }

    #[test]
    fn front_app_profile_overrides_default() {
        let c = profiled();
        assert_eq!(c.action(1, Gesture::Press, SAFARI), (Action::new("url", "https://x.com"), Some("Safari".into())));
        assert_eq!(c.action(2, Gesture::Press, SAFARI).0, Action::none()); // explicitly off in Safari
        assert_eq!(c.action(3, Gesture::Press, SAFARI).1, None); // not in profile: default
        assert_eq!(c.action(1, Gesture::Press, Some("com.other")).0.value.as_deref(), Some("Spotify"));
        assert_eq!(c.action(1, Gesture::Press, None).1, None);
    }

    #[test]
    fn editing_profile_buttons() {
        let mut c = profiled();
        assert_eq!(c.action(3, Gesture::Press, SAFARI).1, None); // inherits
        c.set_action(Some(Action::new("url", "y.com")), 3, Gesture::Press, SAFARI);
        assert_eq!(c.action(3, Gesture::Press, SAFARI).0.value.as_deref(), Some("y.com"));
        assert_eq!(c.buttons.get("3"), None); // default untouched
        c.set_action(None, 3, Gesture::Press, SAFARI);
        assert_eq!(c.action(3, Gesture::Press, SAFARI).1, None);
        c.set_action(Some(Action::new("url", "z.com")), 1, Gesture::Press, Some("com.gone")); // deleted profile: no-op
        assert!(!c.profiles.contains_key("com.gone"));
    }

    #[test]
    fn gesture_keys_inherit_like_presses() {
        let mut c = profiled();
        assert_eq!(c.action(1, Gesture::Long, SAFARI).0.value.as_deref(), Some("Music"));
        c.set_action(Some(Action::none()), 1, Gesture::Long, SAFARI);
        assert_eq!(c.action(1, Gesture::Long, SAFARI).0, Action::none());
        assert_eq!(c.profiles["com.apple.Safari"].buttons.get("1.long"), Some(&Action::none()));
        assert_eq!(c.action(1, Gesture::Double, None).0, Action::none());
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("http://x.com/a"), "http://x.com/a");
        assert_eq!(normalize_url("zoommtg://join"), "zoommtg://join");
    }
}
