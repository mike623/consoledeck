//! deck.json, shared with the Swift app and deck.py.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl Action {
    pub fn none() -> Self {
        Action { kind: "none".into(), value: None }
    }
}

/// Per-app overrides. A button missing here falls back to the default buttons.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub buttons: HashMap<String, Action>,
}

/// Every key is optional so files written before a key existed still load.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub pins: HashMap<String, u8>,
    pub buttons: HashMap<String, Action>, // the default profile; deck.py only knows this one
    pub profiles: HashMap<String, Profile>, // keyed by bundle ID (Mac) / exe name (Windows)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_press_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_press_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_seconds: Option<u64>,
}

impl Config {
    /// Same file as the Swift app on Mac (~/Library/Application Support), %APPDATA% on Windows.
    pub fn path() -> PathBuf {
        dirs::data_dir().expect("no data dir").join("ConsoleDeck").join("deck.json")
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

    /// Default profile's press action for a pin; None when the pin isn't calibrated.
    pub fn action_for_pin(&self, pin: &str) -> Option<(u8, Action)> {
        let n = *self.pins.get(pin)?;
        Some((n, self.buttons.get(&n.to_string()).cloned().unwrap_or_else(Action::none)))
    }
}

/// "example.com" opens as https://example.com, like the Swift app.
pub fn normalize_url(value: &str) -> String {
    if value.contains("://") { value.to_string() } else { format!("https://{value}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_deck_py_config() {
        let c = Config::parse(r#"{"pins": {"D6": 1, "D5": 5}, "buttons": {"1": {"type": "app", "value": "Spotify"}}}"#).unwrap();
        assert_eq!(c.action_for_pin("D6"), Some((1, Action { kind: "app".into(), value: Some("Spotify".into()) })));
        assert_eq!(c.action_for_pin("D5"), Some((5, Action::none())));
        assert_eq!(c.action_for_pin("D2"), None);
    }

    #[test]
    fn reads_swift_app_config() {
        let c = Config::parse(r#"{
            "buttons": {"1": {"type": "url", "value": "x.com"}, "1.long": {"type": "none"}},
            "doublePressMs": 250, "longPressMs": 600, "pins": {"A2": 8},
            "profiles": {"com.apple.Safari": {"buttons": {"1": {"type": "none"}}, "name": "Safari"}},
            "repeatSeconds": 30
        }"#).unwrap();
        assert_eq!(c.long_press_ms, Some(600));
        assert_eq!(c.double_press_ms, Some(250));
        assert_eq!(c.repeat_seconds, Some(30));
        assert_eq!(c.profiles["com.apple.Safari"].name, "Safari");
        assert_eq!(c.action_for_pin("A2"), Some((8, Action::none())));
    }

    #[test]
    fn missing_keys_default_and_broken_json_errors() {
        assert_eq!(Config::parse("{}").unwrap(), Config::default());
        assert!(Config::parse("{\"pins\": ").is_err());
    }

    #[test]
    fn round_trips() {
        let c = Config::parse(r#"{"pins": {"D6": 1}, "buttons": {"1": {"type": "url", "value": "https://x.com"}}}"#).unwrap();
        assert_eq!(Config::parse(&serde_json::to_string(&c).unwrap()).unwrap(), c);
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("http://x.com/a"), "http://x.com/a");
        assert_eq!(normalize_url("zoommtg://join"), "zoommtg://join");
    }
}
