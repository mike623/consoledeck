//! Runs a button's action through its plugin. Slow ones (typing) run on their own thread so the deck keeps listening.

use crate::config::{normalize_url, Action};
use crate::plugins::{self, Plugin, Step};
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

/// Runs a button's plugin action. Repeat, editor and cheat sheet never get here: the deck handles them.
pub fn run(app: &AppHandle, plugins: &[Plugin], action: &Action) {
    if action.kind == "none" {
        return;
    }
    let Some(plugin) = plugins::find(plugins, &action.kind) else {
        return log::error!("No {} plugin installed: add it in Plugins", action.kind);
    };
    let Some(step) = plugin.manifest.step(action) else { return };
    match step {
        Step::Exec(args) => platform::run_plugin(&plugins::resolve(&args, &plugin.dir), &plugin.dir),
        Step::Shell(script) => platform::run_script(&script),
        Step::Url(url) => {
            if let Err(e) = app.opener().open_url(normalize_url(&url), None::<&str>) {
                log::error!("Cannot open {url}: {e}");
            }
        }
        Step::App(name) => platform::open_app(&name),
        Step::Keys(combo) => press_keys(&combo),
        Step::Type(text) => type_text(text),
        Step::Password(sealed) => match passwords::password_key(false).and_then(|k| passwords::decrypt(&sealed, &k)) {
            Some(p) => type_text(p),
            None => log::error!("Cannot decrypt password (secret.key missing or changed); set it again in the editor"),
        },
        // TIS (Mac) must run on the main thread
        Step::Input(id) if id == "next" => {
            let _ = app.run_on_main_thread(platform::next_input_source);
        }
        Step::Input(id) => {
            let _ = app.run_on_main_thread(move || platform::select_input_source(&id));
        }
        Step::Media(key) => match key.as_str() {
            "mute" => media_key(Key::VolumeMute),
            "volumeup" => media_key(Key::VolumeUp),
            "volumedown" => media_key(Key::VolumeDown),
            other => log::error!("Unknown media key {other}"),
        },
    }
}
