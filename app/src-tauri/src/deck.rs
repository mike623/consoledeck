//! The deck's state: presses in, effects out. No I/O here, so it's tested without hardware;
//! main.rs runs the effects (actions, windows) and feeds in lines, ticks and the front app.

use crate::config::{Action, Config, Gesture, POSITIONS};
use crate::gestures::{Gestures, Uses};
use crate::platform::FrontApp;
use crate::plugins::{self, Plugin};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Effect {
    Run(Action),
    PressKeys(String), // a "repeat" tick
    ShowWindow(&'static str),
    ShowCheatSheet,
    HideCheatSheet,
}

/// What the windows and tray show.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub config: Config,
    pub status: String,
    pub last_press: String,
    pub front_app: Option<FrontApp>,
    pub profile_name: String,
    pub recent_apps: Vec<FrontApp>,
    pub calibration: Option<BTreeMap<String, u8>>,
    pub calibration_note: String,
    pub next_to_calibrate: Option<u8>,
    pub repeating: Option<String>,
    pub page: u8,
    pub page_count: u8,
}

pub struct Deck {
    pub config: Config,
    pub status: String,
    pub last_press: String,
    pub front_app: Option<FrontApp>,
    recent_apps: Vec<FrontApp>, // for the editor's "add profile" menu, newest first
    /// Some while calibrating: presses are recorded as pin -> button instead of running actions.
    calibration: Option<BTreeMap<String, u8>>,
    calibration_note: String,
    /// Key combo a "repeat" action is auto-pressing (keeps idle-kicking streams like Boosteroid alive).
    pub repeating: Option<String>,
    repeat_at: Option<Instant>,
    /// Page of buttons in use, from 1; back to 1 when the front app changes.
    pub page: u8,
    held: HashSet<u8>,
    cheat_sheet: bool,
    cheat_holder: Option<u8>, // button whose release hides it
    cheat_hide_at: Option<Instant>,
    gestures: Gestures,
    own_pid: u32,
    load: Box<dyn Fn() -> Result<Config, String> + Send>,
    save: Box<dyn Fn(&Config) -> Result<(), String> + Send>,
    effects: Vec<Effect>,
    pub plugins: Vec<Plugin>,
    /// Latest line from each live command, by its filled-in args.
    pub live: std::collections::HashMap<Vec<String>, String>,
}

impl Deck {
    pub fn new(
        own_pid: u32,
        load: impl Fn() -> Result<Config, String> + Send + 'static,
        save: impl Fn(&Config) -> Result<(), String> + Send + 'static,
    ) -> Deck {
        let mut deck = Deck {
            config: Config::default(),
            status: "Starting...".into(),
            last_press: String::new(),
            front_app: None,
            recent_apps: vec![],
            calibration: None,
            calibration_note: String::new(),
            repeating: None,
            repeat_at: None,
            page: 1,
            held: HashSet::new(),
            cheat_sheet: false,
            cheat_holder: None,
            cheat_hide_at: None,
            gestures: Gestures::default(),
            own_pid,
            load: Box::new(load),
            save: Box::new(save),
            effects: vec![],
            plugins: vec![],
            live: Default::default(),
        };
        deck.reload();
        deck
    }

    /// Picks up edits made by deck.py or by hand; keeps the last good config if the file is broken.
    /// false (and logged) when the file is broken: callers must not save over it then.
    pub fn reload(&mut self) -> bool {
        match (self.load)() {
            Ok(c) => {
                self.config = c;
                true
            }
            Err(e) => {
                log::error!("Cannot read deck.json: {e}");
                false
            }
        }
    }

    /// Forget buttons that are down: their releases may never come (unplug, calibration).
    pub fn reset_presses(&mut self) {
        self.gestures = Gestures::default();
        self.held.clear();
        self.hide_cheat_sheet();
    }

    /// Every edit is saved straight away, like deck.py.
    pub fn save(&mut self) {
        if let Err(e) = (self.save)(&self.config) {
            log::error!("Cannot save deck.json: {e}");
        }
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Buttons down right now, in order.
    pub fn held(&self) -> Vec<u8> {
        let mut h: Vec<u8> = self.held.iter().copied().collect();
        h.sort();
        h
    }

    /// Profile id for the app in front: "com.boosteroid.macclient/Last Epoch" while a plugin says that game is on.
    pub fn profile_id(&self) -> Option<String> {
        self.front().map(|a| a.id)
    }

    /// The app in front with its context, if a plugin reports one: id "<app>/<game>", name "<app> · <game>".
    pub fn front(&self) -> Option<FrontApp> {
        let app = self.front_app.clone()?;
        let context = self.context_command().and_then(|(k, _)| self.live.get(&k)).filter(|c| !c.is_empty());
        Some(match context {
            Some(c) => FrontApp { id: format!("{}/{c}", app.id), name: format!("{} · {c}", app.name), pid: app.pid },
            None => app,
        })
    }

    /// The live command of the plugin that tells the front app's context, while that app is in front.
    pub fn context_command(&self) -> Option<(Vec<String>, std::path::PathBuf)> {
        let app = self.front_app.as_ref()?;
        let p = self.plugins.iter().find(|p| p.manifest.context_for.contains(&app.id))?;
        let action = Action { kind: p.manifest.id.clone(), value: None, values: Default::default() };
        Some((p.manifest.live(&action)?, p.dir.clone()))
    }

    pub fn view(&self) -> View {
        View {
            config: self.config.clone(),
            status: self.status.clone(),
            last_press: self.last_press.clone(),
            front_app: self.front(),
            profile_name: self.config.profile(self.profile_id().as_deref()).map_or("Default".into(), |p| p.name.clone()),
            recent_apps: self.front().filter(|f| Some(f) != self.front_app.as_ref()).into_iter().chain(self.recent_apps.clone()).collect(),
            calibration: self.calibration.clone(),
            calibration_note: self.calibration_note.clone(),
            next_to_calibrate: self.next_to_calibrate(),
            repeating: self.repeating.clone(),
            page: self.page,
            page_count: self.config.page_count(self.profile_id().as_deref()),
        }
    }

    /// ConsoleDeck's own windows must not switch the profile away from the app being configured.
    pub fn set_front_app(&mut self, app: Option<FrontApp>) -> bool {
        let Some(app) = app.filter(|a| a.pid != self.own_pid) else { return false };
        if self.front_app.as_ref() == Some(&app) {
            return false;
        }
        self.recent_apps.retain(|a| a.id != app.id);
        self.recent_apps.insert(0, app.clone());
        self.recent_apps.truncate(12);
        self.front_app = Some(app);
        self.page = 1;
        log::info!("Front app {}", self.front_app.as_ref().unwrap().id);
        true
    }

    // MARK: calibration

    fn next_to_calibrate(&self) -> Option<u8> {
        let cal = self.calibration.as_ref()?;
        POSITIONS.into_iter().find(|n| !cal.values().any(|v| v == n))
    }

    pub fn start_calibration(&mut self) {
        self.reset_presses(); // a held button's long press must not fire mid-calibration
        self.calibration = Some(BTreeMap::new());
        self.calibration_note = "Press button 1".into();
    }

    pub fn cancel_calibration(&mut self) {
        if self.calibration.take().is_some() {
            self.calibration_note = "Cancelled, nothing changed".into();
        }
    }

    fn calibrate(&mut self, pin: &str) {
        let Some(n) = self.next_to_calibrate() else { return };
        let cal = self.calibration.as_mut().unwrap();
        if let Some(existing) = cal.get(pin) {
            self.calibration_note = format!("{pin} is already button {existing}. Press button {n}");
            return;
        }
        cal.insert(pin.into(), n);
        match self.next_to_calibrate() {
            Some(next) => self.calibration_note = format!("Button {n} = {pin}. Press button {next}"),
            None => {
                let pins = self.calibration.take().unwrap();
                if self.reload() {
                    self.config.pins = pins;
                    self.save();
                    self.calibration_note = format!("Saved all {} buttons", POSITIONS.len());
                } else {
                    self.calibration_note = "deck.json is broken; fix it and calibrate again".into();
                }
            }
        }
    }

    // MARK: presses

    pub fn button_event(&mut self, pin: &str, down: bool, now: Instant) {
        if self.calibration.is_some() {
            if down {
                self.calibrate(pin);
            }
            return;
        }
        if down {
            self.reload(); // re-read each press so deck.py or hand edits apply immediately
            self.hide_cheat_sheet(); // any press dismisses it
        }
        let Some(&n) = self.config.pins.get(pin) else {
            if down {
                self.last_press = format!("{pin}: not calibrated");
            }
            return;
        };
        if !down {
            self.held.remove(&n);
            if self.cheat_holder == Some(n) {
                self.hide_cheat_sheet();
            }
            if let Some(g) = self.gestures.up(n, now) {
                self.fire(n, g, now);
            }
            return;
        }
        self.held.insert(n);
        self.gestures.long_press = Duration::from_millis(self.config.long_press_ms.unwrap_or(500));
        self.gestures.double_gap = Duration::from_millis(self.config.double_press_ms.unwrap_or(300));
        let app = self.profile_id();
        let has = |g| self.config.action(self.page, n, g, app.as_deref()).0.kind != "none";
        let uses = Uses { long: has(Gesture::Long), double: has(Gesture::Double) };
        if let Some(g) = self.gestures.down(n, uses, now) {
            self.fire(n, g, now);
        }
    }

    /// Fires gestures, repeats and the cheat sheet timeout that are due.
    pub fn tick(&mut self, now: Instant) {
        for (n, g) in self.gestures.tick(now) {
            self.fire(n, g, now);
        }
        if self.repeat_at.is_some_and(|at| at <= now) {
            self.effects.push(Effect::PressKeys(self.repeating.clone().unwrap()));
            self.repeat_at = Some(now + self.repeat_every());
        }
        if self.cheat_hide_at.is_some_and(|at| at <= now) {
            self.hide_cheat_sheet();
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        [self.gestures.next_deadline(), self.repeat_at, self.cheat_hide_at].into_iter().flatten().min()
    }

    fn fire(&mut self, n: u8, gesture: Gesture, now: Instant) {
        let (action, profile) = self.config.action(self.page, n, gesture, self.profile_id().as_deref());
        let what = match gesture {
            Gesture::Press => "",
            Gesture::Long => " long press",
            Gesture::Double => " double press",
        };
        let summary = format!("button {n}{what} → {}{}", profile.map(|p| format!("{p}: ")).unwrap_or_default(), describe(&action, &self.plugins));
        log::info!("{summary}");
        self.last_press = format!("Last: {summary}");
        match (action.kind.as_str(), action.value.as_deref()) {
            ("repeat", v) => self.toggle_repeat(v.unwrap_or(""), now),
            ("system", Some("editor")) => self.effects.push(Effect::ShowWindow("actions")),
            ("system", Some("cheatsheet")) => self.show_cheat_sheet(n, gesture == Gesture::Long, now),
            ("system", Some(v @ ("nextpage" | "prevpage" | "cyclepage"))) => self.turn_page(v != "prevpage"),
            _ => self.effects.push(Effect::Run(action)),
        }
    }

    /// Long press: up until release. Press and double: up for a few seconds (they may fire while
    /// the button is still down, so "held" alone can't tell).
    fn show_cheat_sheet(&mut self, n: u8, long: bool, now: Instant) {
        self.hide_cheat_sheet();
        self.cheat_sheet = true;
        self.effects.push(Effect::ShowCheatSheet);
        if long && self.held.contains(&n) {
            self.cheat_holder = Some(n);
        } else {
            self.cheat_hide_at = Some(now + Duration::from_secs(4));
        }
    }

    fn hide_cheat_sheet(&mut self) {
        self.cheat_holder = None;
        self.cheat_hide_at = None;
        if std::mem::take(&mut self.cheat_sheet) {
            self.effects.push(Effect::HideCheatSheet);
        }
    }

    /// Wraps around: next on the last page is page 1.
    fn turn_page(&mut self, next: bool) {
        let count = self.config.page_count(self.profile_id().as_deref());
        let page = self.page.min(count); // the page may have been emptied since
        self.page = if next { page % count + 1 } else { (page + count - 2) % count + 1 };
        self.last_press = format!("{} · page {}/{count}", self.last_press, self.page);
    }

    fn repeat_every(&self) -> Duration {
        Duration::from_secs(self.config.repeat_seconds.unwrap_or(60).max(1))
    }

    /// Same combo again stops it; a different repeat button switches to its combo.
    pub fn toggle_repeat(&mut self, combo: &str, now: Instant) {
        let was = self.repeating.take();
        self.repeat_at = None;
        if was.as_deref() == Some(combo) || combo.is_empty() {
            return;
        }
        self.repeating = Some(combo.into());
        self.effects.push(Effect::PressKeys(combo.into()));
        self.repeat_at = Some(now + self.repeat_every());
    }
}

/// One line for the tray and log.
pub fn describe(action: &Action, plugins: &[Plugin]) -> String {
    let value = action.value.as_deref().unwrap_or("");
    if let Some(n) = action.name().filter(|_| action.kind != "none") {
        return n.into();
    }
    match (action.kind.as_str(), value) {
        ("none", _) => "no action".into(),
        ("repeat", _) => format!("repeat {value}"),
        ("system", "editor") => "Open ConsoleDeck Editor".into(),
        ("system", "cheatsheet") => "Show Cheat Sheet".into(),
        ("system", "nextpage") => "Next Page".into(),
        ("system", "prevpage") => "Previous Page".into(),
        ("system", "cyclepage") => "Cycle Page".into(),
        (kind, _) => plugins::find(plugins, kind).map_or(format!("{kind} {value} (plugin not installed)"), |p| p.manifest.describe(action)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const OWN: u32 = 999;

    fn deck(json: &str) -> (Deck, Arc<Mutex<Option<Config>>>) {
        let config = Config::parse(json).unwrap();
        let saved = Arc::new(Mutex::new(None));
        let s = saved.clone();
        (Deck::new(OWN, move || Ok(config.clone()), move |c| Ok(*s.lock().unwrap() = Some(c.clone()))), saved)
    }

    fn app(id: &str, pid: u32) -> Option<FrontApp> {
        Some(FrontApp { id: id.into(), name: id.into(), pid })
    }

    fn press(d: &mut Deck, pin: &str, now: Instant) -> Vec<Effect> {
        d.button_event(pin, true, now);
        d.button_event(pin, false, now);
        d.take_effects()
    }

    const CONFIG: &str = r#"{"pins": {"D2": 1, "D3": 2, "D4": 3},
        "buttons": {"1": {"type": "url", "value": "a.com"}, "2": {"type": "repeat", "value": "space"},
                    "3": {"type": "url", "value": "b.com"}, "3.long": {"type": "system", "value": "cheatsheet"}},
        "profiles": {"com.apple.Safari": {"name": "Safari", "buttons": {"1": {"type": "url", "value": "s.com"}}}},
        "repeatSeconds": 30}"#;

    #[test]
    fn press_runs_profile_action_for_front_app() {
        let (mut d, _) = deck(CONFIG);
        let now = Instant::now();
        assert_eq!(press(&mut d, "D2", now), [Effect::Run(Action::new("url", "a.com"))]);
        d.set_front_app(app("com.apple.Safari", 1));
        assert_eq!(press(&mut d, "D2", now), [Effect::Run(Action::new("url", "s.com"))]);
        assert_eq!(d.last_press, "Last: button 1 → Safari: url s.com (plugin not installed)");
        assert_eq!(press(&mut d, "D9", now), []);
        assert_eq!(d.last_press, "D9: not calibrated");
    }

    #[test]
    fn game_a_plugin_detects_picks_its_profile() {
        let (mut d, _) = deck(r#"{"pins": {"D2": 1}, "buttons": {"1": {"type": "url", "value": "a.com"}},
            "profiles": {"com.boosteroid.macclient/Last Epoch": {"name": "Last Epoch", "buttons": {"1": {"type": "url", "value": "le.com"}}}}}"#);
        let m = r#"{"id": "boosteroid", "name": "B", "version": "1", "contextFor": ["com.boosteroid.macclient"], "live": {"exec": ["./game.sh"]}}"#;
        d.plugins = vec![Plugin { manifest: plugins::Manifest::parse(m).unwrap(), dir: Default::default() }];
        assert!(d.context_command().is_none()); // only while the app is in front
        d.set_front_app(app("com.boosteroid.macclient", 1));
        let (key, _) = d.context_command().unwrap();
        assert_eq!(d.profile_id().as_deref(), Some("com.boosteroid.macclient"));
        d.live.insert(key.clone(), "Last Epoch".into());
        assert_eq!(d.profile_id().as_deref(), Some("com.boosteroid.macclient/Last Epoch"));
        assert_eq!(d.view().profile_name, "Last Epoch");
        assert_eq!(d.view().recent_apps[0].name, "com.boosteroid.macclient · Last Epoch"); // to add its profile
        assert_eq!(press(&mut d, "D2", Instant::now()), [Effect::Run(Action::new("url", "le.com"))]);
        d.live.insert(key, "".into()); // stream ended
        assert_eq!(d.profile_id().as_deref(), Some("com.boosteroid.macclient"));
    }

    #[test]
    fn own_windows_keep_previous_front_app() {
        let (mut d, _) = deck(CONFIG);
        assert!(d.set_front_app(app("com.apple.Safari", 1)));
        assert!(!d.set_front_app(app("com.consoledeck.app", OWN)));
        assert!(!d.set_front_app(None));
        assert_eq!(d.profile_id().as_deref(), Some("com.apple.Safari"));
        d.set_front_app(app("com.apple.Finder", 2));
        d.set_front_app(app("com.apple.Safari", 1));
        let recent: Vec<_> = d.view().recent_apps.into_iter().map(|a| a.id).collect();
        assert_eq!(recent, ["com.apple.Safari", "com.apple.Finder"]);
    }

    #[test]
    fn calibration_maps_pins_in_order_and_saves() {
        let (mut d, saved) = deck(CONFIG);
        let now = Instant::now();
        d.start_calibration();
        assert_eq!(d.view().next_to_calibrate, Some(1));
        assert_eq!(press(&mut d, "D7", now), []); // swallowed, not run
        press(&mut d, "D7", now);
        assert_eq!(d.view().calibration_note, "D7 is already button 1. Press button 2");
        for i in 2..=10 {
            press(&mut d, &format!("A{i}"), now);
        }
        let pins = saved.lock().unwrap().clone().unwrap().pins;
        assert_eq!((pins["D7"], pins["A10"], pins.len()), (1, 10, 10));
        assert_eq!(d.view().calibration, None);
        assert_eq!(d.view().calibration_note, "Saved all 10 buttons");
    }

    #[test]
    fn cancel_calibration_restores_presses() {
        let (mut d, saved) = deck(CONFIG);
        d.start_calibration();
        d.cancel_calibration();
        assert_eq!(press(&mut d, "D2", Instant::now()).len(), 1);
        assert!(saved.lock().unwrap().is_none());
    }

    #[test]
    fn repeat_toggles_and_ticks() {
        let (mut d, _) = deck(CONFIG);
        let t0 = Instant::now();
        assert_eq!(press(&mut d, "D3", t0), [Effect::PressKeys("space".into())]);
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_secs(30)));
        d.tick(t0 + Duration::from_secs(30));
        assert_eq!(d.take_effects(), [Effect::PressKeys("space".into())]);
        assert_eq!(press(&mut d, "D3", t0), []); // stops
        assert_eq!((d.repeating.clone(), d.next_deadline()), (None, None));
    }

    #[test]
    fn cheat_sheet_stays_while_held_and_any_press_hides_it() {
        let (mut d, _) = deck(CONFIG);
        let t0 = Instant::now();
        d.button_event("D4", true, t0);
        d.tick(t0 + Duration::from_millis(600)); // long press of button 3
        assert_eq!(d.take_effects(), [Effect::ShowCheatSheet]);
        d.button_event("D4", false, t0 + Duration::from_secs(9));
        assert_eq!(d.take_effects(), [Effect::HideCheatSheet]);
        // shown from somewhere else, not held: hides on its own after 4s
        d.show_cheat_sheet(1, false, t0);
        d.take_effects();
        d.tick(t0 + Duration::from_secs(4));
        assert_eq!(d.take_effects(), [Effect::HideCheatSheet]);
        d.show_cheat_sheet(1, false, t0);
        d.take_effects();
        assert_eq!(press(&mut d, "D2", t0)[0], Effect::HideCheatSheet);
    }

    #[test]
    fn cheat_sheet_on_plain_press_outlives_the_release() {
        let (mut d, _) = deck(r#"{"pins": {"D2": 1}, "buttons": {"1": {"type": "system", "value": "cheatsheet"}}}"#);
        let t0 = Instant::now();
        assert_eq!(press(&mut d, "D2", t0), [Effect::ShowCheatSheet]); // fires on down, released right after
        d.tick(t0 + Duration::from_secs(4));
        assert_eq!(d.take_effects(), [Effect::HideCheatSheet]);
    }

    #[test]
    fn calibration_and_unplug_drop_pending_gestures() {
        let (mut d, _) = deck(CONFIG);
        let t0 = Instant::now();
        d.button_event("D4", true, t0); // button 3 has a long action: waiting
        d.start_calibration();
        d.tick(t0 + Duration::from_secs(1));
        assert_eq!(d.take_effects(), []);
        d.cancel_calibration();
        d.button_event("D4", true, t0);
        d.reset_presses();
        d.tick(t0 + Duration::from_secs(1));
        assert_eq!(d.take_effects(), []);
    }

    #[test]
    fn broken_config_is_never_saved_over() {
        let saved = Arc::new(Mutex::new(None));
        let s = saved.clone();
        let mut d = Deck::new(OWN, || Err("trailing comma".into()), move |c| Ok(*s.lock().unwrap() = Some(c.clone())));
        d.start_calibration();
        for i in 1..=10 {
            press(&mut d, &format!("D{i}"), Instant::now());
        }
        assert!(saved.lock().unwrap().is_none());
        assert_eq!(d.view().calibration_note, "deck.json is broken; fix it and calibrate again");
    }

    #[test]
    fn page_buttons_turn_pages_and_wrap() {
        let (mut d, _) = deck(r#"{"pins": {"D2": 1, "D3": 2}, "buttons": {
            "1": {"type": "system", "value": "nextpage"}, "2": {"type": "url", "value": "one.com"},
            "p2.2": {"type": "url", "value": "two.com"},
            "p3.1": {"type": "system", "value": "prevpage"}, "p3.2": {"type": "system", "value": "nextpage"}}}"#);
        let now = Instant::now();
        assert_eq!(press(&mut d, "D2", now), []);
        assert_eq!(d.view().page, 2);
        assert_eq!(d.last_press, "Last: button 1 → Next Page · page 2/3");
        assert_eq!(press(&mut d, "D3", now), [Effect::Run(Action::new("url", "two.com"))]);
        press(&mut d, "D2", now); // nothing on page 2 button 1: stays
        assert_eq!(d.page, 2);
        d.page = 3;
        press(&mut d, "D3", now); // next on the last page wraps
        assert_eq!(d.page, 1);
        d.page = 3;
        press(&mut d, "D2", now);
        assert_eq!(d.page, 2);
        d.set_front_app(app("com.apple.Safari", 1));
        assert_eq!(d.page, 1);
    }

    #[test]
    fn cycle_page_holds_its_button_on_every_page() {
        let (mut d, _) = deck(r#"{"pins": {"D2": 1, "D3": 2}, "buttons": {
            "1": {"type": "system", "value": "cyclepage"}, "p2.1": {"type": "url", "value": "hidden.com"},
            "p3.2": {"type": "url", "value": "three.com"}}}"#);
        let now = Instant::now();
        press(&mut d, "D2", now);
        assert_eq!(d.page, 2);
        press(&mut d, "D2", now); // page 2's own button 1 is covered by Cycle Page
        assert_eq!(d.page, 3);
        assert_eq!(press(&mut d, "D3", now), [Effect::Run(Action::new("url", "three.com"))]);
        press(&mut d, "D2", now); // wraps
        assert_eq!(d.page, 1);
    }

    #[test]
    fn editor_action_opens_window() {
        let (mut d, _) = deck(r#"{"pins": {"D2": 1}, "buttons": {"1": {"type": "system", "value": "editor"}}}"#);
        assert_eq!(press(&mut d, "D2", Instant::now()), [Effect::ShowWindow("actions")]);
    }

    #[test]
    fn describes_actions() {
        let plugins: Vec<Plugin> = [
            r#"{"id": "system", "name": "System", "version": "1", "fields": [{"id": "action", "label": "Action", "type": "select",
                "options": [{"value": "restart", "label": "Restart…", "run": {"exec": ["x"]}}]}]}"#,
            r#"{"id": "password", "name": "Type Password", "version": "1", "fields": [{"id": "p", "label": "P", "type": "password"}], "run": {"password": "{p}"}}"#,
        ].iter().map(|j| Plugin { manifest: plugins::Manifest::parse(j).unwrap(), dir: Default::default() }).collect();
        assert_eq!(describe(&Action::new("system", "restart"), &plugins), "Restart…");
        assert_eq!(describe(&Action::new("system", "cheatsheet"), &plugins), "Show Cheat Sheet");
        assert_eq!(describe(&Action::new("password", "xyz"), &plugins), "Type Password");
        assert_eq!(describe(&Action::new("url", "x.com"), &plugins), "url x.com (plugin not installed)");
        assert_eq!(describe(&Action::none(), &plugins), "no action");
        let mut named = Action::new("system", "nextpage");
        named.values.insert("name".into(), "More".into());
        assert_eq!(describe(&named, &plugins), "More");
    }
}
