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
    /// inkdash's API token (its settings page → API access) for `inkdash_name`; empty = no Kindle.
    pub inkdash_token: String,
    pub inkdash_name: String, // the data shows up as {{<name>.…}}, the theme is called <name>
    pub inkdash_url: String, // empty = inkdash on this Mac
    pub inkdash_theme: String, // version of the theme last uploaded; a new one is also switched to
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, token: new_token(), port: 47800, inkdash_token: String::new(), inkdash_name: "consoledeck".into(), inkdash_url: String::new(), inkdash_theme: String::new() }
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
    dark: bool,
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
    own: bool,            // set by the front app's profile, not the default
    live: Option<String>, // the press action's live line, shown instead of its label (Boosteroid's AFK countdown)
}

/// What the phone shows live: each display tile, then each button's press action on this page.
fn live_actions(d: &Deck) -> Vec<Action> {
    let app = d.profile_id();
    let keys = POSITIONS.iter().map(|&n| d.config.action(d.page, n, Gesture::Press, app.as_deref()).0);
    d.config.displays(app.as_deref()).into_iter().chain(keys).collect()
}

/// The live command (args, plugin folder) behind each display tile and live button.
pub fn live_commands(d: &Deck) -> Vec<(Vec<String>, std::path::PathBuf)> {
    live_actions(d)
        .iter()
        .filter_map(|a| {
            let p = plugins::find(&d.plugins, &a.kind)?;
            Some((p.manifest.live(a)?, p.dir.clone()))
        })
        .collect()
}

/// Latest line of an action's live command, cut to fit; None without one or before its first line.
/// A repeat button that's running shows "AFK on · every 30s".
fn live_line(d: &Deck, a: &Action) -> Option<String> {
    if a.kind == "repeat" {
        return (d.repeating.is_some() && d.repeating == a.value).then(|| format!("AFK on · every {}", d.repeat_label()));
    }
    let key = plugins::find(&d.plugins, &a.kind)?.manifest.live(a)?;
    d.live.get(&key).map(|l| crate::live::clip(l))
}

pub fn view(d: &Deck) -> MobileView {
    let app = d.profile_id();
    let app = app.as_deref();
    let action = |n, g| d.config.action(d.page, n, g, app);
    let label = |n, g| short(&action(n, g).0, &d.plugins);
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
        dark: d.dark,
        keys: POSITIONS
            .iter()
            .map(|&n| {
                let (press, profile) = action(n, Gesture::Press);
                KeyView {
                    n,
                    press: short(&press, &d.plugins),
                    long: label(n, Gesture::Long),
                    double: label(n, Gesture::Double),
                    own: profile.is_some(),
                    live: live_line(d, &press),
                }
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
                DisplayView { label, live: live_line(d, a) }
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
        "system" if ["editor", "cheatsheet", "nextpage", "prevpage", "cyclepage"].contains(&value.as_str()) => return describe(a, plugins), // built in, not in the plugin
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

/// What the Kindle shows, as {{consoledeck.…}} in the inkdash theme: the phone's view, minus what changes
/// on every press (held keys, last press) since each change redraws e-ink. Keys in 3-3-4 rows: themes have no logic.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KindleView {
    profile: String,
    front_app: Option<String>,
    rows: Vec<KindleRow>,
    displays: Vec<DisplayView>,
}

#[derive(Serialize)]
struct KindleRow {
    keys: Vec<KindleKey>,
}

#[derive(Serialize)]
struct KindleKey {
    label: String,
    live: bool,
}

fn kindle_view(v: MobileView) -> KindleView {
    // "26%\t0.26": the phone draws a bar from the part after the tab, the Kindle shows only the text
    let text = |l: String| l.split('\t').next().unwrap_or("").to_string();
    let mut keys = v.keys.into_iter().map(|k| KindleKey { live: k.live.is_some(), label: k.live.map(text).unwrap_or(k.press) });
    let rows = [3, 3, 4].iter().map(|&n| KindleRow { keys: keys.by_ref().take(n).collect() }).collect();
    let displays = v.displays.into_iter().map(|d| DisplayView { label: d.label, live: d.live.map(text) }).collect();
    KindleView { profile: v.profile, front_app: v.front_app, rows, displays }
}

const KINDLE_HTML: &str = include_str!("../../ui/kindle.html");
const KINDLE_CSS: &str = include_str!("../../ui/kindle.css");

/// One request to inkdash's API ("PUT /inkdash/theme/consoledeck"), returns the status line.
/// Plain std HTTP, like the phone server.
fn inkdash(url: &str, token: &str, method_path: &str, json: &str) -> std::io::Result<String> {
    let url = if url.is_empty() { "127.0.0.1:47291" } else { url };
    let host = url.trim_start_matches("http://").split('/').next().unwrap_or("");
    let addr = std::net::ToSocketAddrs::to_socket_addrs(host)?.next().ok_or(std::io::ErrorKind::NotFound)?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let req = format!(
        "{method_path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
        json.len()
    );
    s.write_all(req.as_bytes())?;
    let mut status = String::new();
    BufReader::new(s).read_line(&mut status)?;
    Ok(status.trim().to_string())
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
    kindle: Sender<String>,
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
        let settings = Arc::new(Mutex::new(settings));
        // Kindle pusher: newest state only, at most every 5 s (e-ink redraws are slow and flicker).
        // Uploads the theme first, once per launch, so an update brings its new look.
        let (kindle, krx) = channel::<String>();
        let ks = settings.clone();
        std::thread::spawn(move || {
            let ok = |r: &std::io::Result<String>| matches!(r, Ok(s) if s.split(' ').nth(1).is_some_and(|c| c.starts_with('2')));
            let theme = {
                use std::hash::{DefaultHasher, Hash, Hasher};
                let mut h = DefaultHasher::new();
                (KINDLE_HTML, KINDLE_CSS).hash(&mut h);
                format!("{:x}", h.finish())
            };
            let (mut last, mut theme_sent) = (String::new(), false);
            while let Ok(mut json) = krx.recv() {
                while let Ok(newer) = krx.try_recv() {
                    json = newer;
                }
                if json == last {
                    continue;
                }
                let (url, token, name, shown) = {
                    let s = ks.lock().unwrap();
                    (s.inkdash_url.clone(), s.inkdash_token.clone(), s.inkdash_name.clone(), s.inkdash_theme.clone())
                };
                if !theme_sent {
                    // switch to it only when it's new, so a theme picked on inkdash's settings page stays
                    let body = serde_json::json!({ "template": KINDLE_HTML.replace("consoledeck}}", &format!("{name}}}}}")), "css": KINDLE_CSS, "activate": shown != theme });
                    let r = inkdash(&url, &token, &format!("PUT /inkdash/theme/{name}"), &body.to_string());
                    if ok(&r) {
                        theme_sent = true;
                        let mut s = ks.lock().unwrap();
                        s.inkdash_theme = theme.clone();
                        s.save();
                    } else {
                        log::warn!("inkdash theme upload to {url}: {r:?}");
                    }
                }
                let r = inkdash(&url, &token, &format!("POST /inkdash/push/{name}"), &json);
                if ok(&r) {
                    last = json;
                } else {
                    log::warn!("inkdash push to {url}: {r:?}");
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        let on = settings.lock().unwrap().enabled;
        let c = Companion { on: Arc::new(AtomicBool::new(on)), settings, started: Mutex::new(false), tx, kindle };
        if c.on.load(Ordering::Relaxed) {
            c.start();
        }
        c
    }

    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// A Kindle is set up: companion.json has inkdash's token.
    pub fn pushing(&self) -> bool {
        !self.settings.lock().unwrap().inkdash_token.is_empty()
    }

    pub fn send(&self, d: &Deck) {
        if self.enabled() {
            let _ = self.tx.send(Msg::State(serde_json::to_string(&view(d)).unwrap_or_default()));
        }
        if self.pushing() {
            let _ = self.kindle.send(serde_json::to_string(&kindle_view(view(d))).unwrap_or_default());
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

    /// The same page on this Mac, for the menu bar preview.
    pub fn local_url(&self) -> String {
        let s = self.settings.lock().unwrap();
        format!("http://127.0.0.1:{}/?t={}", s.port, s.token)
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
    fn kindle_rows_are_3_3_4() {
        let json = r#"{"buttons":{"1":{"type":"app","value":"Spotify"}},"pins":{"D2":1}}"#;
        let mut d = Deck::new(0, move || Config::parse(json), |_| Ok(()));
        d.plugins = crate::plugins::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/plugins")));
        let k = kindle_view(view(&d));
        assert_eq!(k.rows.iter().map(|r| r.keys.len()).collect::<Vec<_>>(), [3, 3, 4]);
        assert_eq!(k.rows[0].keys[0].label, "Spotify");
        let out = serde_json::to_string(&k).unwrap();
        assert!(!out.contains("held") && !out.contains("lastPress"), "{out}");
        d.config.displays = vec![Action::new("cpu", "")];
        let key = live_commands(&d)[0].0.clone();
        d.live.insert(key, "26%\t0.26".into());
        assert_eq!(kindle_view(view(&d)).displays[0].live.as_deref(), Some("26%"));
    }

    #[test]
    fn live_commands_come_from_displays_and_buttons() {
        let json = r#"{"buttons":{"1":{"type":"cpu"},"1.long":{"type":"network"}},"displays":[{"type":"memory"},{"type":"gone"}],"pins":{"D2":1}}"#;
        let mut d = Deck::new(0, move || Config::parse(json), |_| Ok(()));
        d.plugins = crate::plugins::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../plugins/plugins")));
        let live = live_commands(&d);
        assert_eq!(live.len(), 2, "{live:?}"); // the tile and button 1's press, not its long press
        assert!(live[0].1.ends_with("memory") && live[1].1.ends_with("cpu"), "{live:?}");
        let out = serde_json::to_string(&view(&d)).unwrap();
        assert!(out.contains(r#""displays":[{"label":"RAM","live":null},{"label":"gone (not installed)","live":null}]"#), "{out}");
        d.live.insert(live[1].0.clone(), "26%\t0.26".into());
        assert!(serde_json::to_string(&view(&d)).unwrap().contains(r#""own":false,"live":"26%\t0.26"}"#));
        if !cfg!(target_os = "macos") { return; } // le-character is mac only
        d.config.displays = vec![Action::new("le-character", "sheet.Fire Resistance")];
        let live = live_commands(&d);
        assert_eq!(live[0].0[2], "sheet.Fire Resistance");
        assert!(serde_json::to_string(&view(&d)).unwrap().contains(r#""label":"Fire Res""#));
    }
}
