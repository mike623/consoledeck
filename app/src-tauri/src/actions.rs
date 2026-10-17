//! Runs a button's action. Slow ones (typing) run on their own thread so the deck keeps listening.

use crate::config::{normalize_url, Action};
use crate::{keys, passwords, platform};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// Input actions need macOS Accessibility. macOS's own prompt is shown once per run, not on every
/// press (a Repeat Keys action would pop it up over a game every interval).
fn input_allowed(what: &str) -> bool {
    static PROMPTED: AtomicBool = AtomicBool::new(false);
    let ok = keys::accessibility_trusted(!PROMPTED.swap(true, Ordering::Relaxed));
    if !ok {
        log::error!("{what} not sent: ConsoleDeck needs Accessibility permission");
    }
    ok
}

pub fn press_keys(combo: &str) {
    let Some(combo) = keys::KeyCombo::parse(combo) else { return log::error!("Unknown key combo {combo}") };
    if input_allowed("Key") {
        if let Err(e) = keys::press(&combo) {
            log::error!("Key not sent: {e}");
        }
    }
}

fn media_key(key: Key) {
    if input_allowed("Volume key") {
        let sent = Enigo::new(&Settings::default()).map_err(|e| e.to_string());
        if let Err(e) = sent.and_then(|mut e| e.key(key, Direction::Click).map_err(|e| e.to_string())) {
            log::error!("Volume key not sent: {e}");
        }
    }
}

fn type_text(text: String) {
    if input_allowed("Text") {
        std::thread::spawn(move || {
            if let Err(e) = keys::type_text(&text) {
                log::error!("Text not typed: {e}");
            }
        });
    }
}

pub fn run(app: &AppHandle, action: &Action) {
    let Some(value) = action.value.clone().filter(|v| !v.is_empty()) else { return };
    match (action.kind.as_str(), value.as_str()) {
        ("url", _) => {
            if let Err(e) = app.opener().open_url(normalize_url(&value), None::<&str>) {
                log::error!("Cannot open {value}: {e}");
            }
        }
        ("app", _) => platform::open_app(&value),
        ("script", _) => platform::run_script(&value),
        ("key", _) => press_keys(&value),
        ("type", _) => type_text(value),
        ("password", _) => match passwords::password_key(false).and_then(|k| passwords::decrypt(&value, &k)) {
            Some(p) => type_text(p),
            None => log::error!("Cannot decrypt password (secret.key missing or changed); set it again in the editor"),
        },
        // TIS (Mac) must run on the main thread
        ("input", _) => {
            let _ = app.run_on_main_thread(move || platform::select_input_source(&value));
        }
        ("system", "inputsource") => {
            let _ = app.run_on_main_thread(platform::next_input_source);
        }
        #[cfg(target_os = "macos")]
        ("system", "lock") => press_keys("ctrl+cmd+q"), // macOS's own Lock Screen shortcut
        #[cfg(target_os = "macos")]
        ("vpn", _) => platform::toggle_vpn(&value),
        ("system", "mute") => media_key(Key::VolumeMute),
        ("system", "volumeup") => media_key(Key::VolumeUp),
        ("system", "volumedown") => media_key(Key::VolumeDown),
        // The deck handles these; from the editor's Test button there's nothing useful to do
        ("system", "editor" | "cheatsheet") => {}
        ("system", id) => platform::run_system_command(id),
        (kind, _) => log::warn!("Unknown action type {kind}"),
    }
}
