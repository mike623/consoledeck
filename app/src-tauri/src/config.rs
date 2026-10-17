//! deck.json, shared with deck.py.

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
    pub value: Option<String>, // the plugin's first field; deck.py reads only this
    /// The plugin's other fields, by field id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, String>,
}

impl Action {
    #[cfg(test)]
    pub fn new(kind: &str, value: &str) -> Self {
        Action { kind: kind.into(), value: Some(value.into()), values: BTreeMap::new() }
    }

    pub fn none() -> Self {
        Action { kind: "none".into(), value: None, values: BTreeMap::new() }
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
    /// Key in deck.json's buttons: "3", "3.long", "3.double", and "p2.3", "p2.3.long" on page 2.
    /// deck.py only knows plain "3".
    pub fn key(self, page: u8, n: u8) -> String {
        let p = if page > 1 { format!("p{page}.") } else { String::new() };
        match self {
            Gesture::Press => format!("{p}{n}"),
            Gesture::Long => format!("{p}{n}.long"),
            Gesture::Double => format!("{p}{n}.double"),
        }
    }
}

/// Page a buttons key is on: "p2.3" is 2, "3" is 1.
fn page_of(key: &str) -> u8 {
    key.strip_prefix('p').and_then(|k| k.split('.').next()?.parse().ok()).unwrap_or(1)
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
    /// Tiles only the phone companion shows (live plugins like CPU Usage), apart from the buttons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<Action>,
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
    /// ~/.config/consoledeck on Mac (not ~/Library/Application Support, which uninstallers delete with the
    /// app), %APPDATA%\ConsoleDeck on Windows. Not $XDG_CONFIG_HOME: apps opened from Finder don't see it.
    pub fn path() -> PathBuf {
        #[cfg(target_os = "macos")]
        let dir = dirs::home_dir().expect("no home folder").join(".config").join("consoledeck");
        #[cfg(not(target_os = "macos"))]
        let dir = dirs::config_dir().expect("no config folder").join("ConsoleDeck");
        dir.join("deck.json")
    }

    /// One-time move of deck.json and secret.key from where older versions kept them.
    pub fn migrate() {
        let home = dirs::home_dir().expect("no home folder");
        let old = [home.join(".consoledeck"), dirs::data_dir().expect("no data folder").join("ConsoleDeck")];
        move_settings(&old, Self::path().parent().unwrap());
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
    pub fn set_action(&mut self, action: Option<Action>, page: u8, n: u8, gesture: Gesture, profile: Option<&str>) {
        let buttons = match profile {
            None => &mut self.buttons,
            Some(p) => match self.profiles.get_mut(p) {
                Some(profile) => &mut profile.buttons,
                None => return,
            },
        };
        match action {
            Some(a) => buttons.insert(gesture.key(page, n), a),
            None => buttons.remove(&gesture.key(page, n)),
        };
    }

    /// Front app's profile first, then the default buttons. The name is None when the default was used.
    /// Pages fall back the same way: page 2 of a profile, then page 2 of the default buttons.
    pub fn action(&self, page: u8, n: u8, gesture: Gesture, app: Option<&str>) -> (Action, Option<String>) {
        let key = gesture.key(page, n);
        if let Some(profile) = app.and_then(|a| self.profiles.get(a)) {
            if let Some(action) = profile.buttons.get(&key) {
                return (action.clone(), Some(profile.name.clone()));
            }
        }
        (self.buttons.get(&key).cloned().unwrap_or_else(Action::none), None)
    }

    /// Highest page with a button set, in the default buttons or the app's profile.
    pub fn page_count(&self, app: Option<&str>) -> u8 {
        let profile = app.and_then(|a| self.profiles.get(a)).map(|p| &p.buttons);
        self.buttons.keys().chain(profile.into_iter().flat_map(|b| b.keys())).map(|k| page_of(k)).max().unwrap_or(1)
    }
}

/// "example.com" opens as https://example.com, like the Swift app.
pub fn normalize_url(value: &str) -> String {
    if value.contains("://") { value.to_string() } else { format!("https://{value}") }
}

fn move_settings(old_dirs: &[PathBuf], new: &std::path::Path) {
    if new.join("deck.json").exists() {
        return;
    }
    let Some(old) = old_dirs.iter().find(|d| d.as_path() != new && d.join("deck.json").exists()) else { return };
    let moved = std::fs::create_dir_all(new).and_then(|_| {
        for f in ["deck.json", "secret.key"].map(|f| old.join(f)).iter().filter(|f| f.exists()) {
            std::fs::rename(f, new.join(f.file_name().unwrap()))?;
        }
        Ok(())
    });
    match moved {
        Ok(()) => {
            let _ = std::fs::remove_dir(old); // only if nothing else is in it
            log::info!("Moved settings from {} to {}", old.display(), new.display())
        }
        Err(e) => log::error!("Cannot move settings from {} to {}: {e}", old.display(), new.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAFARI: Option<&str> = Some("com.apple.Safari");

    #[test]
    fn moves_settings_from_first_old_folder_once() {
        let t = std::env::temp_dir().join(format!("consoledeck-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (a, b, new) = (t.join("a"), t.join("b"), t.join("config/consoledeck"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("deck.json"), d.to_str().unwrap()).unwrap();
        }
        std::fs::write(b.join("secret.key"), "k").unwrap();
        move_settings(&[t.join("missing"), b.clone(), a.clone()], &new);
        assert_eq!(std::fs::read_to_string(new.join("deck.json")).unwrap(), b.to_str().unwrap());
        assert_eq!(std::fs::read_to_string(new.join("secret.key")).unwrap(), "k");
        assert!(!b.exists() && a.exists());
        move_settings(&[a.clone()], &new); // already there: left alone
        assert_eq!(std::fs::read_to_string(new.join("deck.json")).unwrap(), b.to_str().unwrap());
        std::fs::remove_dir_all(&t).unwrap();
    }

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
        assert_eq!(c.action(1, 1, Gesture::Press, None).0, Action::new("app", "Spotify"));
        assert_eq!(c.action(1, 5, Gesture::Press, None).0, Action::none());
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
        assert_eq!(c.action(1, 1, Gesture::Press, SAFARI), (Action::new("url", "https://x.com"), Some("Safari".into())));
        assert_eq!(c.action(1, 2, Gesture::Press, SAFARI).0, Action::none()); // explicitly off in Safari
        assert_eq!(c.action(1, 3, Gesture::Press, SAFARI).1, None); // not in profile: default
        assert_eq!(c.action(1, 1, Gesture::Press, Some("com.other")).0.value.as_deref(), Some("Spotify"));
        assert_eq!(c.action(1, 1, Gesture::Press, None).1, None);
    }

    #[test]
    fn editing_profile_buttons() {
        let mut c = profiled();
        assert_eq!(c.action(1, 3, Gesture::Press, SAFARI).1, None); // inherits
        c.set_action(Some(Action::new("url", "y.com")), 1, 3, Gesture::Press, SAFARI);
        assert_eq!(c.action(1, 3, Gesture::Press, SAFARI).0.value.as_deref(), Some("y.com"));
        assert_eq!(c.buttons.get("3"), None); // default untouched
        c.set_action(None, 1, 3, Gesture::Press, SAFARI);
        assert_eq!(c.action(1, 3, Gesture::Press, SAFARI).1, None);
        c.set_action(Some(Action::new("url", "z.com")), 1, 1, Gesture::Press, Some("com.gone")); // deleted profile: no-op
        assert!(!c.profiles.contains_key("com.gone"));
    }

    #[test]
    fn gesture_keys_inherit_like_presses() {
        let mut c = profiled();
        assert_eq!(c.action(1, 1, Gesture::Long, SAFARI).0.value.as_deref(), Some("Music"));
        c.set_action(Some(Action::none()), 1, 1, Gesture::Long, SAFARI);
        assert_eq!(c.action(1, 1, Gesture::Long, SAFARI).0, Action::none());
        assert_eq!(c.profiles["com.apple.Safari"].buttons.get("1.long"), Some(&Action::none()));
        assert_eq!(c.action(1, 1, Gesture::Double, None).0, Action::none());
    }

    #[test]
    fn pages_have_their_own_buttons_and_inherit_like_page_one() {
        let mut c = profiled();
        assert_eq!((c.page_count(None), c.page_count(SAFARI)), (1, 1));
        c.set_action(Some(Action::new("url", "p2.com")), 2, 1, Gesture::Long, None);
        c.set_action(Some(Action::new("url", "s3.com")), 3, 4, Gesture::Press, SAFARI);
        assert_eq!(c.buttons["p2.1.long"], Action::new("url", "p2.com"));
        assert_eq!((c.page_count(None), c.page_count(SAFARI), c.page_count(Some("com.other"))), (2, 3, 2));
        assert_eq!(c.action(2, 1, Gesture::Long, SAFARI).0.value.as_deref(), Some("p2.com")); // from default page 2
        assert_eq!(c.action(2, 1, Gesture::Press, None).0, Action::none()); // page 1's button 1 isn't on page 2
        assert_eq!(c.action(3, 4, Gesture::Press, None).0, Action::none());
        assert_eq!(page_of("p12.3.double"), 12);
    }

    #[test]
    fn extra_plugin_fields_round_trip_in_values() {
        let json = r#"{"buttons": {"1": {"type": "vpn", "value": "Home", "values": {"mode": "fast"}}, "2": {"type": "url", "value": "x.com"}}}"#;
        let c = Config::parse(json).unwrap();
        assert_eq!(c.buttons["1"].values["mode"], "fast");
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains(r#""values":{"mode":"fast"}"#) && !saved.contains(r#""x.com","values""#));
        assert_eq!(Config::parse(&saved).unwrap(), c);
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("http://x.com/a"), "http://x.com/a");
        assert_eq!(normalize_url("zoommtg://join"), "zoommtg://join");
    }
}
