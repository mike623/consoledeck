//! Phone companion: a page on the LAN that shows the deck live (display only, no presses).
//! Plain std HTTP on a fixed port, so a home screen web app keeps working across restarts.
//! Pairing is the token in the QR code's URL; "Unpair" makes a new one.
//! State goes out as server-sent events: one-way, and EventSource reconnects by itself.

use crate::config::{Action, Config, Gesture, POSITIONS};
use crate::deck::{describe, Deck};
use crate::plugins::{self, Plugin};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PAGE: &str = include_str!("../../ui/mobile.html");
/// Changes when an update changes the page, so a home screen web app (no reload button) can offer one.
static PAGE_VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    PAGE.hash(&mut h);
    format!("{:x}", h.finish())
});
const ICON: &[u8] = include_bytes!("../icons/128x128@2x.png");

/// companion.json next to deck.json (deck.py rewrites deck.json and would drop these).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub token: String,
    pub port: u16, // fixed so the phone's home screen URL stays valid; edit the file if it's taken
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, token: new_token(), port: 47800 }
    }
}

impl Settings {
    fn path() -> std::path::PathBuf {
        Config::path().with_file_name("companion.json")
    }

    pub fn load() -> Settings {
        let s: Settings = std::fs::read_to_string(Self::path()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        if !Self::path().exists() {
            s.save(); // keep the first token, or every restart would unpair the phone
        }
        s
    }

    pub fn save(&self) {
        let written = std::fs::create_dir_all(Self::path().parent().unwrap())
            .and_then(|_| std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap_or_default()));
        if let Err(e) = written {
            log::error!("Cannot save companion.json: {e}");
        }
    }
}

fn new_token() -> String {
    use aes_gcm::aead::{Generate, Key};
    Key::<aes_gcm::Aes256Gcm>::generate()[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// What the phone shows. Labels only, never the actions themselves (scripts, sealed passwords).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileView {
    page: &'static str,
    profile: String,
    front_app: Option<String>,
    status: String,
    last_press: String,
    repeating: Option<String>,
    held: Vec<u8>,
    keys: Vec<KeyView>,
    displays: Vec<DisplayView>,
}

#[derive(Serialize)]
struct DisplayView {
    label: String,
    live: Option<String>, // latest line of the plugin's live command, e.g. "23%"; None until the first one
}

#[derive(Serialize)]
struct KeyView {
    n: u8,
    press: String,
    long: String,
    double: String,
    own: bool, // set by the front app's profile, not the default
}

/// The live command (args, plugin folder) behind each display tile.
pub fn live_commands(d: &Deck) -> Vec<(Vec<String>, std::path::PathBuf)> {
    d.config
        .displays(d.profile_id().as_deref())
        .iter()
        .filter_map(|a| {
            let p = plugins::find(&d.plugins, &a.kind)?;
            Some((p.manifest.live(a)?, p.dir.clone()))
        })
        .collect()
}

pub fn view(d: &Deck) -> MobileView {
    let app = d.profile_id();
    let app = app.as_deref();
    let label = |n, g| {
        let (a, profile) = d.config.action(d.page, n, g, app);
        (short(&a, &d.plugins), profile.is_some())
    };
    MobileView {
        page: &PAGE_VERSION,
        profile: match d.view() {
            v if v.page_count > 1 => format!("{} · Page {}", v.profile_name, v.page),
            v => v.profile_name,
        },
        front_app: d.front().map(|a| a.name),
        status: d.status.clone(),
        last_press: d.last_press.clone(),
        repeating: d.repeating.clone(),
        held: d.held(),
        keys: POSITIONS
            .iter()
            .map(|&n| {
                let (press, own) = label(n, Gesture::Press);
                KeyView { n, press, long: label(n, Gesture::Long).0, double: label(n, Gesture::Double).0, own }
            })
            .collect(),
        displays: d
            .config
            .displays(app)
            .iter()
            .map(|a| {
                let p = plugins::find(&d.plugins, &a.kind);
                // a tile with a value is one of the plugin's choices: "Fire Res", not "Last Epoch Character"
                let choice = |p: &Plugin| p.manifest.fields.first()?.options.iter().find(|o| a.value.as_ref() == Some(&o.value)).map(|o| o.label.clone());
                let label = p.map_or_else(
                    || format!("{} (not installed)", a.kind),
                    |p| choice(p).or(p.manifest.label.clone()).unwrap_or(p.manifest.name.clone()),
                );
                DisplayView { label, live: p.and_then(|p| p.manifest.live(a)).and_then(|k| d.live.get(&k).map(|l| crate::live::clip(l))) }
            })
            .collect(),
    }
}

/// Like the cheat sheet's label: "youtube.com", "Spotify", not "Open URL \"https://www.youtube.com/\"".
fn short(a: &Action, plugins: &[Plugin]) -> String {
    if let Some(n) = a.name() {
        return n.into();
    }
    let value = a.value.clone().unwrap_or_default();
    match a.kind.as_str() {
        "system" if ["editor", "cheatsheet", "nextpage", "prevpage"].contains(&value.as_str()) => return describe(a, plugins), // built in, not in the plugin
        // ponytail: "com.apple.keylayout.British" -> "British"; real names need the main thread on macOS
        "input" if value != "next" => return value.rsplit('.').next().unwrap_or("").into(),
        _ => {}
    }
    let Some(field) = plugins::find(plugins, &a.kind).and_then(|p| p.manifest.fields.first().map(|f| (p, f))) else {
        return if a.kind == "none" { String::new() } else { describe(a, plugins) };
    };
    let (p, field) = field;
    if let Some(o) = field.options.iter().find(|o| o.value == value) {
        return o.label.clone();
    }
    match field.kind.as_str() {
        "password" => p.manifest.name.clone(),
        "url" => value.split("://").last().unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").into(),
        _ => value.chars().take(40).collect(),
    }
}

enum Msg {
    Client(TcpStream),
    State(String),
    Drop, // turned off: close every phone's stream
}

/// Shared with the windows: on/off, token, and where phones send state.
pub struct Companion {
    pub settings: Arc<Mutex<Settings>>,
    on: Arc<AtomicBool>,
    started: Mutex<bool>,
    tx: Sender<Msg>,
}

impl Companion {
    pub fn new() -> Companion {
        let settings = Settings::load();
        let (tx, rx) = channel::<Msg>();
        // One thread owns the phone streams, so a slow phone never blocks the deck.
        std::thread::spawn(move || {
            let mut clients: Vec<TcpStream> = vec![];
            let mut last = String::new();
            loop {
                let ping = ": ping\n\n".to_string(); // finds phones that went away
                let (out, to_all) = match rx.recv_timeout(Duration::from_secs(20)) {
                    Ok(Msg::Client(mut s)) => {
                        if s.write_all(format!("data: {last}\n\n").as_bytes()).is_ok() {
                            clients.push(s);
                        }
                        continue;
                    }
                    Ok(Msg::State(json)) if json == last => continue,
                    Ok(Msg::State(json)) => {
                        last = json;
                        (format!("data: {last}\n\n"), true)
                    }
                    Ok(Msg::Drop) => {
                        clients.clear();
                        continue;
                    }
                    Err(RecvTimeoutError::Timeout) => (ping, true),
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                if to_all {
                    clients.retain_mut(|s| s.write_all(out.as_bytes()).and_then(|_| s.flush()).is_ok());
                }
            }
        });
        let c = Companion { on: Arc::new(AtomicBool::new(settings.enabled)), settings: Arc::new(Mutex::new(settings)), started: Mutex::new(false), tx };
        if c.on.load(Ordering::Relaxed) {
            c.start();
        }
        c
    }

    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    pub fn send(&self, d: &Deck) {
        if self.enabled() {
            let _ = self.tx.send(Msg::State(serde_json::to_string(&view(d)).unwrap_or_default()));
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
        let mut s = self.settings.lock().unwrap();
        s.enabled = on;
        s.save();
        drop(s);
        if on {
            self.start();
        } else {
            let _ = self.tx.send(Msg::Drop);
        }
    }

    /// New token: phones paired before get "not paired" until they scan again.
    pub fn unpair(&self) {
        let mut s = self.settings.lock().unwrap();
        s.token = new_token();
        s.save();
        drop(s);
        let _ = self.tx.send(Msg::Drop);
    }

    pub fn url(&self) -> String {
        let s = self.settings.lock().unwrap();
        format!("http://{}:{}/?t={}", host(), s.port, s.token)
    }

    /// Binds once; turning off later keeps the port but refuses every request.
    /// ponytail: never unbinds, add a shutdown path if the open port matters while off.
    fn start(&self) {
        let mut started = self.started.lock().unwrap();
        if *started {
            return;
        }
        let port = self.settings.lock().unwrap().port;
        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => return log::error!("Phone companion cannot use port {port}: {e}"),
        };
        log::info!("Phone companion on port {port}");
        *started = true;
        let (on, tx, settings) = (self.on.clone(), self.tx.clone(), self.settings.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !on.load(Ordering::Relaxed) {
                    continue; // dropped: closed
                }
                let tx = tx.clone();
                let token = settings.lock().unwrap().token.clone();
                std::thread::spawn(move || handle(stream, &token, &tx));
            }
        });
    }
}

/// The Mac's Bonjour name survives DHCP and VPNs; elsewhere the LAN address.
fn host() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("scutil").args(["--get", "LocalHostName"]).output() {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !name.is_empty() {
            return format!("{name}.local");
        }
    }
    // ponytail: route to a private address picks the LAN interface; wrong on LANs outside 192.168/16 behind a VPN
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.168.0.1:9").and_then(|_| s.local_addr()))
        .map_or("localhost".into(), |a| a.ip().to_string())
}

/// "GET /events?t=abc HTTP/1.1" -> ("/events", Some("abc")).
fn parse_request(line: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = line.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let token = query.split('&').find_map(|kv| kv.strip_prefix("t="));
    Some((path, token))
}

/// Same time whatever the mismatch, so the token can't be guessed byte by byte.
fn token_ok(given: Option<&str>, token: &str) -> bool {
    let Some(given) = given else { return false };
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn respond(s: &mut TcpStream, status: &str, kind: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body));
}

fn handle(mut s: TcpStream, token: &str, tx: &Sender<Msg>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    let Ok(reader) = s.try_clone() else { return };
    let mut lines = BufReader::new(reader.take(8192)).lines(); // caps the headers too
    let Some(Ok(first)) = lines.next() else { return };
    for line in lines.by_ref() {
        if line.map_or(true, |l| l.is_empty()) {
            break;
        }
    }
    let Some((path, given)) = parse_request(&first) else { return respond(&mut s, "405 Method Not Allowed", "text/plain", b"") };
    let ok = token_ok(given, token);
    match (path, ok) {
        ("/icon.png", _) => respond(&mut s, "200 OK", "image/png", ICON),
        ("/", true) => respond(&mut s, "200 OK", "text/html; charset=utf-8", PAGE.replace("{{TOKEN}}", token).replace("{{PAGE}}", &PAGE_VERSION).as_bytes()),
        ("/manifest.webmanifest", true) => {
            let m = serde_json::json!({
                "name": "ConsoleDeck", "short_name": "Deck", "start_url": format!("/?t={token}"), "display": "fullscreen",
                "orientation": "any", "background_color": "#000000", "theme_color": "#000000",
                "icons": [{ "src": "/icon.png", "sizes": "256x256", "type": "image/png" }],
            });
            respond(&mut s, "200 OK", "application/manifest+json", m.to_string().as_bytes())
        }
        ("/events", true) => {
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n";
            if s.write_all(head.as_bytes()).is_ok() {
                let _ = tx.send(Msg::Client(s));
            }
        }
        ("/" | "/manifest.webmanifest" | "/events", false) => respond(
            &mut s,
            "403 Forbidden",
            "text/html; charset=utf-8",
            b"<meta name=viewport content='width=device-width'><body style='font:17px system-ui;padding:24px'>\
              <h3>Not paired</h3>Scan the QR code in ConsoleDeck's menu: <b>Phone Companion\xe2\x80\xa6</b>",
        ),
        _ => respond(&mut s, "404 Not Found", "text/plain", b""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_tokens() {
        assert_eq!(parse_request("GET /events?x=1&t=ab HTTP/1.1"), Some(("/events", Some("ab"))));
        assert_eq!(parse_request("GET / HTTP/1.1"), Some(("/", None)));
        assert_eq!(parse_request("POST / HTTP/1.1"), None);
        assert!(token_ok(Some("abcd"), "abcd"));
        assert!(!token_ok(Some("abce"), "abcd"));
        assert!(!token_ok(Some("abc"), "abcd"));
        assert!(!token_ok(None, "abcd"));
        assert_eq!(new_token().len(), 32);
    }

    #[test]
    fn view_never_carries_secrets() {
        let json = r#"{"buttons":{"1":{"type":"script","value":"rm -rf ~/x","values":{"secret":"hunter2"}},
            "2":{"type":"password","value":"SEALEDCIPHERTEXT"}},"pins":{"D2":1}}"#;
        let mut d = Deck::new(0, move || Config::parse(json), |_| Ok(()));
        d.plugins = crate::plugins::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/plugins")));
        let out = serde_json::to_string(&view(&d)).unwrap();
        assert!(!out.contains("hunter2") && !out.contains("SEALEDCIPHERTEXT"), "{out}");
        assert!(out.contains(r#""n":10"#));
        let a = |kind: &str, v: &str| short(&Action { kind: kind.into(), value: Some(v.into()), values: Default::default() }, &d.plugins);
        assert_eq!(a("url", "https://www.youtube.com/watch"), "youtube.com");
        assert_eq!(a("app", "Spotify"), "Spotify");
        assert_eq!(a("none", ""), "");
        assert_eq!(a("input", "com.apple.keylayout.British"), "British");
        assert_eq!(a("system", "editor"), "Open ConsoleDeck Editor");
        let mut named = Action::new("script", "uv run --project ~/le-tool le-scanner overlay");
        assert_eq!(short(&named, &d.plugins), "uv run --project ~/le-tool le-scanner ov"); // cut at 40
        named.values.insert("name".into(), " Scanner ".into());
        assert_eq!(short(&named, &d.plugins), "Scanner");
    }

    #[test]
    fn live_commands_come_from_displays_not_buttons() {
        let json = r#"{"buttons":{"1":{"type":"cpu"}},"displays":[{"type":"memory"},{"type":"gone"}],"pins":{"D2":1}}"#;
        let mut d = Deck::new(0, move || Config::parse(json), |_| Ok(()));
        d.plugins = crate::plugins::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/plugins")));
        let live = live_commands(&d);
        assert_eq!(live.len(), 1);
        assert!(live[0].1.ends_with("memory"), "{live:?}");
        let out = serde_json::to_string(&view(&d)).unwrap();
        assert!(out.contains(r#""displays":[{"label":"RAM","live":null},{"label":"gone (not installed)","live":null}]"#), "{out}");
        d.config.displays = vec![Action::new("le-character", "sheet.Fire Resistance")];
        let live = live_commands(&d);
        assert_eq!(live[0].0[2], "sheet.Fire Resistance");
        assert!(serde_json::to_string(&view(&d)).unwrap().contains(r#""label":"Fire Res""#));
    }
}
