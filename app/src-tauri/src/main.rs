// No console window behind the tray app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod serial;

use config::{normalize_url, Action, Config};
use std::thread::sleep;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};
use tauri_plugin_opener::OpenerExt;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // menu bar only, no Dock icon
            let status = MenuItem::with_id(app, "status", "Starting...", false, None::<&str>)?;
            let last = MenuItem::with_id(app, "last", "No presses yet", false, None::<&str>)?;
            let menu = MenuBuilder::new(app)
                .items(&[&status, &last])
                .separator()
                .text("quit", "Quit ConsoleDeck")
                .build()?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("bundle icon").clone())
                .tooltip("ConsoleDeck")
                .menu(&menu)
                .on_menu_event(|app, e| {
                    if e.id() == "quit" {
                        app.exit(0)
                    }
                })
                .build(app)?;
            let app = app.handle().clone();
            std::thread::spawn(move || listen_forever(app, status, last));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ConsoleDeck");
}

fn listen_forever(app: AppHandle, status: MenuItem<Wry>, last: MenuItem<Wry>) {
    let report = |msg: &str| {
        log::info!("{msg}");
        let _ = status.set_text(msg);
    };
    let mut config = Config::default();
    let mut shown = String::new(); // retries every 3s only report when the problem changes
    loop {
        let result = match serial::pick_port(&serial::usb_ports(), std::env::var("DECK_PORT").ok()) {
            None => Err("Waiting for deck (plug in USB)".to_string()),
            Some(path) => serial::open(&path).and_then(|port| {
                shown = format!("Listening on {path}");
                report(&shown);
                serial::read_lines(port, &path, |line| on_line(&app, &last, &mut config, line))
            }),
        };
        if let Err(e) = result {
            if e != shown {
                report(&e);
                shown = e;
            }
        }
        sleep(Duration::from_secs(3));
    }
}

fn on_line(app: &AppHandle, last: &MenuItem<Wry>, config: &mut Config, line: &str) {
    // ponytail: press only; release matters once gestures land (#3)
    let Some((pin, true)) = serial::parse_button(line) else { return };
    // Re-read each press so deck.py or hand edits apply immediately; keep the last good config if broken.
    match Config::load() {
        Ok(c) => *config = c,
        Err(e) => log::error!("Cannot read deck.json: {e}"),
    }
    let Some((n, action)) = config.action_for_pin(&pin) else {
        let _ = last.set_text(format!("{pin}: not calibrated"));
        return;
    };
    let summary = format!("button {n} → {}", describe(&action));
    log::info!("{summary}");
    let _ = last.set_text(format!("Last: {summary}"));
    run(app, &action);
}

fn describe(action: &Action) -> String {
    match action.kind.as_str() {
        "none" => "no action".into(),
        kind => format!("{kind} {}", action.value.as_deref().unwrap_or("")),
    }
}

fn run(app: &AppHandle, action: &Action) {
    match (action.kind.as_str(), action.value.as_deref()) {
        ("none", _) => {}
        ("url", Some(v)) => {
            if let Err(e) = app.opener().open_url(normalize_url(v), None::<&str>) {
                log::error!("Cannot open {v}: {e}")
            }
        }
        (kind, _) => log::warn!("{kind} actions are not supported yet"),
    }
}
