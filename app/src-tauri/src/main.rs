// No console window behind the tray app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod config;
mod deck;
mod gestures;
mod keys;
mod passwords;
mod platform;
mod serial;

use config::{Action, Config, Gesture};
use deck::{Deck, Effect};
use serde::Serialize;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

/// How often the app in front is checked.
const POLL: Duration = Duration::from_millis(300);

struct Tray {
    status: MenuItem<Wry>,
    profile: MenuItem<Wry>,
    last: MenuItem<Wry>,
    repeat: MenuItem<Wry>,
    login: CheckMenuItem<Wry>,
}

struct Shared {
    deck: Mutex<Deck>,
    tray: Tray,
    shown: Mutex<String>, // last view pushed to the tray and windows, to skip no-op refreshes
}

enum Event {
    Button(String, bool),
    Status(String),
}

/// Changes the deck, then runs what it asked for and refreshes the tray and windows.
fn update<R>(app: &AppHandle, f: impl FnOnce(&mut Deck) -> R) -> R {
    let shared = app.state::<Shared>();
    let (r, effects, view) = {
        let mut deck = shared.deck.lock().unwrap();
        let r = f(&mut deck);
        (r, deck.take_effects(), deck.view())
    };
    // windows render the new state before the cheat sheet appears
    let json = serde_json::to_string(&view).unwrap_or_default();
    if *shared.shown.lock().unwrap() != json {
        *shared.shown.lock().unwrap() = json;
        let t = &shared.tray;
        let _ = t.status.set_text(&view.status);
        let front = view.front_app.as_ref().map(|a| format!("  ({} in front)", a.name)).unwrap_or_default();
        let _ = t.profile.set_text(format!("Profile: {}{front}", view.profile_name));
        let _ = t.last.set_text(if view.last_press.is_empty() { "No presses yet" } else { &view.last_press });
        let _ = t.repeat.set_text(view.repeating.as_ref().map_or("Not repeating".into(), |c| format!("Stop Repeating {c}")));
        let _ = t.repeat.set_enabled(view.repeating.is_some());
        let _ = app.emit("state", ());
    }
    for e in effects {
        match e {
            Effect::Run(a) => actions::run(app, &a),
            Effect::PressKeys(c) => actions::press_keys(&c),
            Effect::ShowWindow(label) => show_window(app, label),
            Effect::ShowCheatSheet => cheat_sheet(app, true),
            Effect::HideCheatSheet => cheat_sheet(app, false),
        }
    }
    r
}

fn show_window(app: &AppHandle, label: &str) {
    let window = app.get_webview_window(label).or_else(|| {
        let (title, w, h) = if label == "calibrate" { ("Calibrate ConsoleDeck", 380.0, 400.0) } else { ("ConsoleDeck Actions", 440.0, 540.0) };
        WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("{label}.html").into()))
            .title(title)
            .inner_size(w, h)
            .min_inner_size(w, 300.0)
            .center()
            .build()
            .map_err(|e| log::error!("Cannot open {label}: {e}"))
            .ok()
    });
    if let Some(w) = window {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    #[cfg(target_os = "macos")]
    let _ = app.show(); // menu bar apps don't come to the front on their own
}

/// Floating, click-through, non-focusable panel, so the front app (and its profile) keeps focus.
fn cheat_sheet(app: &AppHandle, show: bool) {
    let window = app.get_webview_window("cheatsheet").or_else(|| {
        let w = WebviewWindowBuilder::new(app, "cheatsheet", WebviewUrl::App("cheatsheet.html".into()))
            .title("ConsoleDeck Cheat Sheet")
            .inner_size(620.0, 360.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true) // above full-screen games and streams
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()
            .map_err(|e| log::error!("Cannot open cheat sheet: {e}"))
            .ok()?;
        let _ = w.set_ignore_cursor_events(true);
        Some(w)
    });
    if let Some(w) = window {
        if show {
            let _ = w.center();
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    }
}

fn listen_forever(tx: Sender<Event>) {
    let mut shown = String::new(); // retries every 3s only report when the problem changes
    let mut report = |msg: String| {
        if msg != shown {
            log::info!("{msg}");
            shown = msg.clone();
            let _ = tx.send(Event::Status(msg));
        }
    };
    loop {
        let result = match serial::pick_port(&serial::usb_ports(), std::env::var("DECK_PORT").ok()) {
            None => Err("Waiting for deck (plug in USB)".to_string()),
            Some(path) => serial::open(&path).and_then(|port| {
                report(format!("Listening on {path}"));
                serial::read_lines(port, &path, |line| {
                    if let Some((pin, down)) = serial::parse_button(line) {
                        let _ = tx.send(Event::Button(pin, down));
                    }
                })
            }),
        };
        if let Err(e) = result {
            report(e);
        }
        sleep(Duration::from_secs(3));
    }
}

/// Feeds presses, timers and the app in front into the deck.
fn deck_loop(app: AppHandle, rx: Receiver<Event>) {
    loop {
        let next = app.state::<Shared>().deck.lock().unwrap().next_deadline();
        let wait = next.map_or(POLL, |t| t.saturating_duration_since(Instant::now()).min(POLL));
        let event = rx.recv_timeout(wait).ok();
        let front = platform::front_app();
        update(&app, |d| {
            match event {
                Some(Event::Button(pin, down)) => d.button_event(&pin, down, Instant::now()),
                Some(Event::Status(s)) => {
                    d.reset_presses(); // connection changed: releases from before won't come
                    d.status = s;
                }
                None => {}
            }
            d.tick(Instant::now());
            d.set_front_app(front);
        });
    }
}

// MARK: commands for the windows

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct State {
    #[serde(flatten)]
    view: deck::View,
    accessibility: bool,
    mac: bool,
}

#[tauri::command]
fn get_state(app: AppHandle) -> State {
    State {
        view: app.state::<Shared>().deck.lock().unwrap().view(),
        accessibility: keys::accessibility_trusted(false),
        mac: cfg!(target_os = "macos"),
    }
}

#[tauri::command]
fn set_action(app: AppHandle, profile: Option<String>, gesture: Gesture, button: u8, action: Option<Action>) {
    update(&app, |d| {
        if d.reload() {
            d.config.set_action(action, button, gesture, profile.as_deref());
            d.save();
        }
    })
}

#[tauri::command]
fn add_profile(app: AppHandle, id: String, name: String) {
    update(&app, |d| {
        if d.reload() {
            d.config.profiles.entry(id).or_insert(config::Profile { name, ..Default::default() });
            d.save();
        }
    })
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) {
    update(&app, |d| {
        if d.reload() {
            d.config.profiles.remove(&id);
            d.save();
        }
    })
}

#[tauri::command]
fn start_calibration(app: AppHandle) {
    update(&app, Deck::start_calibration)
}

#[tauri::command]
fn cancel_calibration(app: AppHandle) {
    update(&app, Deck::cancel_calibration)
}

#[tauri::command]
fn test_action(app: AppHandle, action: Action) {
    actions::run(&app, &action)
}

/// Encrypted form to store in deck.json; the key is created on first use.
#[tauri::command]
fn seal_password(password: String) -> Option<String> {
    passwords::encrypt(&password, &passwords::password_key(true)?)
}

#[derive(Serialize)]
struct PickedApp {
    id: String,
    name: String,
    open: String, // what an "Open App" action stores
}

#[tauri::command]
async fn pick_app(app: AppHandle) -> Option<PickedApp> {
    let dialog = app.dialog().file().set_title("Choose App");
    #[cfg(target_os = "macos")]
    let dialog = dialog.set_directory("/Applications").add_filter("Applications", &["app"]);
    #[cfg(windows)]
    let dialog = dialog.add_filter("Programs", &["exe"]);
    let path = dialog.blocking_pick_file()?.into_path().ok()?;
    let (id, name) = platform::app_identity(&path)?;
    let open = if cfg!(target_os = "macos") { name.clone() } else { path.to_string_lossy().into_owned() };
    Some(PickedApp { id, name, open })
}

/// Sync commands run on the main thread, which macOS's input source API needs.
#[tauri::command]
fn input_sources() -> Vec<(String, String)> {
    platform::input_sources()
}

#[tauri::command]
fn system_actions() -> Vec<(&'static str, &'static str)> {
    platform::SYSTEM_ACTIONS.to_vec()
}

#[tauri::command]
fn allow_accessibility(app: AppHandle) {
    if !keys::accessibility_trusted(true) {
        let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
        let _ = app.opener().open_url(url, None::<&str>);
    }
}

/// Asks before installing a newer release from GitHub, then restarts into it. Blocks: run it on its own thread.
/// `manual` (from the tray) also says when there's nothing new or the check failed.
fn check_for_update(app: &AppHandle, manual: bool) {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
    use tauri_plugin_updater::UpdaterExt;
    let tell = |text: String, kind| {
        app.dialog().message(text).title("ConsoleDeck").kind(kind).blocking_show();
    };
    let found = tauri::async_runtime::block_on(async { app.updater()?.check().await });
    let update = match found {
        Ok(Some(u)) => u,
        Ok(None) => return if manual { tell(format!("ConsoleDeck {} is the latest version.", app.package_info().version), MessageDialogKind::Info) },
        Err(e) => {
            log::warn!("Update check failed: {e}");
            return if manual { tell(format!("Cannot check for updates: {e}"), MessageDialogKind::Error) };
        }
    };
    let install = app
        .dialog()
        .message(format!("ConsoleDeck {} is available (you have {}). Install it and restart?", update.version, update.current_version))
        .title("ConsoleDeck Update")
        .buttons(MessageDialogButtons::OkCancelCustom("Install".into(), "Later".into()))
        .blocking_show();
    if !install {
        return;
    }
    log::info!("Installing ConsoleDeck {}", update.version);
    match tauri::async_runtime::block_on(update.download_and_install(|_, _| {}, || {})) {
        Ok(()) => app.restart(),
        Err(e) => {
            log::error!("Update failed: {e}");
            tell(format!("Update failed: {e}"), MessageDialogKind::Error);
        }
    }
}

fn main() {
    let (tx, rx) = channel();
    let builder = tauri::Builder::default();
    // Edit menu only: paste works in text fields, and ⌘Q / ⌘W reach the key recorder instead of quitting.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(|app| {
        use tauri::menu::SubmenuBuilder;
        let edit = SubmenuBuilder::new(app, "Edit").undo().redo().separator().cut().copy().paste().select_all().build()?;
        MenuBuilder::new(app).item(&SubmenuBuilder::new(app, "ConsoleDeck").build()?).item(&edit).build()
    });
    let app = builder
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_state, set_action, add_profile, delete_profile, start_calibration, cancel_calibration,
            test_action, seal_password, pick_app, input_sources, system_actions, allow_accessibility
        ])
        .on_window_event(|w, e| {
            // closing the window must not leave presses swallowed
            if w.label() == "calibrate" && matches!(e, WindowEvent::Destroyed) {
                update(w.app_handle(), Deck::cancel_calibration);
            }
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // menu bar only, no Dock icon
            let item = |id: &str, text: &str, enabled| MenuItem::with_id(app, id, text, enabled, None::<&str>);
            let login_on = app.autolaunch().is_enabled().unwrap_or(false);
            let tray = Tray {
                status: item("status", "Starting...", false)?,
                profile: item("profile", "Profile: Default", false)?,
                last: item("last", "No presses yet", false)?,
                repeat: item("repeat", "Not repeating", false)?,
                login: CheckMenuItem::with_id(app, "login", "Start at Login", true, login_on, None::<&str>)?,
            };
            let reveal = if cfg!(target_os = "macos") { "Show Config in Finder" } else { "Show Config in Explorer" };
            let menu = MenuBuilder::new(app)
                .items(&[&tray.status, &tray.profile, &tray.last, &tray.repeat])
                .separator()
                .text("actions", "Edit Actions…")
                .text("calibrate", "Calibrate Buttons…")
                .separator()
                .item(&tray.login)
                .text("config", reveal)
                .text("update", "Check for Updates…")
                .text("quit", "Quit ConsoleDeck")
                .build()?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("bundle icon").clone())
                .tooltip("ConsoleDeck")
                .menu(&menu)
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "actions" | "calibrate" => show_window(app, e.id().as_ref()),
                    "repeat" => update(app, |d| {
                        let combo = d.repeating.clone().unwrap_or_default();
                        d.toggle_repeat(&combo, Instant::now())
                    }),
                    "login" => {
                        let al = app.autolaunch();
                        let on = !al.is_enabled().unwrap_or(false);
                        if let Err(e) = if on { al.enable() } else { al.disable() } {
                            log::error!("Start at login {on} failed: {e}");
                        }
                        let _ = app.state::<Shared>().tray.login.set_checked(al.is_enabled().unwrap_or(false));
                    }
                    "config" => {
                        if let Err(e) = app.opener().reveal_item_in_dir(Config::path()) {
                            log::error!("Cannot show config: {e}");
                        }
                    }
                    "update" => {
                        let app = app.clone();
                        std::thread::spawn(move || check_for_update(&app, true));
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            let deck = Deck::new(std::process::id(), Config::load, Config::save);
            app.manage(Shared { deck: Mutex::new(deck), tray, shown: Mutex::new(String::new()) });
            std::thread::spawn(move || listen_forever(tx));
            let handle = app.handle().clone();
            std::thread::spawn(move || deck_loop(handle, rx));
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                check_for_update(&handle, false);
                std::thread::sleep(Duration::from_secs(24 * 60 * 60)); // a tray app can run for weeks
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building ConsoleDeck");
    app.run(|_, event| {
        // closing the last window must not quit a tray app
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
