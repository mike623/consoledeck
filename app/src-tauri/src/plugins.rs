//! Action plugins: a folder per plugin beside deck.json holding plugin.json and its scripts.
//! A button's "type" is the plugin id. The manifest says which fields the editor shows and what a
//! press runs: one of the plugin's commands (exec) or something ConsoleDeck does itself (keys, url...).
//! The registry is github.com/mike623/consoledeck-plugins; its README documents the format.

use crate::config::{Action, Config};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// "mac", "windows"; empty means both
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    /// Shown under the fields in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Deck label, "{field}" placeholders; default is the first field's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What a press does when the chosen select option has no run of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    pub label: String,
    /// text, textarea, url, app, keys, password, select
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// Select options listed at edit time: a command printing "value" or "value<TAB>label" lines,
    /// or ["inputSources"] for the enabled input languages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_from: Option<Vec<String>>,
    /// A button beside the field whose command's last output line becomes the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<FieldButton>,
    #[serde(default)]
    pub monospace: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldButton {
    pub label: String,
    /// File extensions to pick first; the path is "{file}" in exec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pick_file: Vec<String>,
    pub exec: Vec<String>,
}

/// Exactly one way to act, or per-OS variants under "mac" / "windows". Strings take "{field}" placeholders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Run {
    /// Program and arguments, no shell; "./x" is a file in the plugin folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<Vec<String>>,
    /// A command line the user typed, run by bash (Mac) / cmd (Windows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_text: Option<String>,
    /// Sealed password (as the editor stores it), typed decrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Input source id; "next" for the one after the current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// mute, volumeup, volumedown
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<Box<Run>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Box<Run>>,
}

/// What a press does, placeholders filled in.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Exec(Vec<String>),
    Shell(String),
    Url(String),
    App(String),
    Keys(String),
    Type(String),
    Password(String),
    Input(String),
    Media(String),
}

pub const OS: &str = if cfg!(target_os = "macos") { "mac" } else { "windows" };

fn on_this_os(platforms: &[String]) -> bool {
    platforms.is_empty() || platforms.iter().any(|p| p == OS)
}

impl Run {
    /// This OS's variant, then its single step. None when it does nothing here.
    fn step(&self, fill: &impl Fn(&str) -> String) -> Option<Step> {
        let os = if cfg!(target_os = "macos") { &self.mac } else { &self.windows };
        if let Some(r) = os {
            return r.step(fill);
        }
        let s = |v: &Option<String>| v.as_deref().map(fill);
        let step = if let Some(args) = &self.exec {
            Step::Exec(args.iter().map(|a| fill(a)).collect())
        } else if let Some(v) = s(&self.shell) {
            Step::Shell(v)
        } else if let Some(v) = s(&self.url) {
            Step::Url(v)
        } else if let Some(v) = s(&self.app) {
            Step::App(v)
        } else if let Some(v) = s(&self.keys) {
            Step::Keys(v)
        } else if let Some(v) = s(&self.type_text) {
            Step::Type(v)
        } else if let Some(v) = s(&self.password) {
            Step::Password(v)
        } else if let Some(v) = s(&self.input) {
            Step::Input(v)
        } else {
            Step::Media(s(&self.media)?)
        };
        Some(step)
    }

    fn ways(&self) -> usize {
        [self.exec.is_some(), self.shell.is_some(), self.url.is_some(), self.app.is_some(), self.keys.is_some(),
         self.type_text.is_some(), self.password.is_some(), self.input.is_some(), self.media.is_some()]
            .iter().filter(|b| **b).count()
    }

    fn check(&self) -> Result<(), String> {
        let variants = self.mac.is_some() || self.windows.is_some();
        match (self.ways(), variants) {
            (1, false) | (0, true) => {}
            _ => return Err("run needs exactly one of exec, shell, url, app, keys, type, password, input, media (or mac/windows variants)".into()),
        }
        if self.exec.as_ref().is_some_and(|e| e.is_empty()) {
            return Err("exec is empty".into());
        }
        self.mac.iter().chain(&self.windows).try_for_each(|r| r.check())
    }
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !valid_id(&m.id) {
            return Err(format!("bad id {:?}: use a-z, 0-9 and -", m.id));
        }
        for f in &m.fields {
            if !["text", "textarea", "url", "app", "keys", "password", "select"].contains(&f.kind.as_str()) {
                return Err(format!("field {}: unknown type {}", f.id, f.kind));
            }
            f.options.iter().filter_map(|o| o.run.as_ref()).try_for_each(Run::check)?;
        }
        m.run.iter().try_for_each(Run::check)?;
        let option_runs = m.fields.first().is_some_and(|f| f.options.iter().any(|o| o.run.is_some()));
        if m.run.is_none() && !option_runs {
            return Err("nothing to run: add run".into());
        }
        Ok(m)
    }

    /// Value of each field: the first is the action's value, the rest its values map.
    fn values(&self, action: &Action) -> BTreeMap<String, String> {
        let mut v = action.values.clone();
        if let (Some(first), Some(value)) = (self.fields.first(), &action.value) {
            v.insert(first.id.clone(), value.clone());
        }
        v
    }

    /// What pressing a button with this action does here. None when nothing is set or it doesn't apply.
    pub fn step(&self, action: &Action) -> Option<Step> {
        let values = self.values(action);
        let first = self.fields.first().and_then(|f| values.get(&f.id)).filter(|v| !v.is_empty());
        if !self.fields.is_empty() && first.is_none() {
            return None;
        }
        let option = self.fields.first().and_then(|f| f.options.iter().find(|o| Some(&o.value) == first));
        if option.is_some_and(|o| !on_this_os(&o.platforms)) {
            return None;
        }
        let run = option.and_then(|o| o.run.as_ref()).or(self.run.as_ref())?;
        run.step(&|s| fill(s, &values))
    }

    /// One line for the tray and log.
    pub fn describe(&self, action: &Action) -> String {
        let Some(field) = self.fields.first() else { return self.name.clone() };
        let value = action.value.clone().unwrap_or_default();
        if let Some(o) = field.options.iter().find(|o| o.value == value) {
            return o.label.clone();
        }
        match field.kind.as_str() {
            "password" => self.name.clone(),
            _ => format!("{} \"{}\"", self.name, value.chars().take(30).collect::<String>()),
        }
    }
}

/// "{name}" -> that field's value; unknown names become empty.
fn fill(template: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let name = &rest[start + 1..start + len];
        out.push_str(&rest[..start]);
        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !name.is_empty() {
            out.push_str(values.get(name).map_or("", String::as_str));
        } else {
            out.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    out + rest
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Plain relative path inside a plugin folder: no "..", no root, no drive.
pub fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty() && !path.contains('\\') && p.components().all(|c| matches!(c, Component::Normal(_)))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plugin {
    #[serde(flatten)]
    pub manifest: Manifest,
    #[serde(skip)]
    pub dir: PathBuf,
}

pub fn dir() -> PathBuf {
    Config::path().parent().unwrap().join("plugins")
}

/// Installed plugins for this OS, by id. Broken ones are logged and skipped.
pub fn load(dir: &Path) -> Vec<Plugin> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut plugins: Vec<Plugin> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let file = e.path().join("plugin.json");
            let manifest = std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|j| Manifest::parse(&j));
            match manifest {
                Ok(m) if m.id != e.file_name().to_string_lossy() => {
                    log::error!("Plugin {}: id {} doesn't match its folder", file.display(), m.id);
                    None
                }
                Ok(m) => on_this_os(&m.platforms).then(|| Plugin { manifest: m, dir: e.path() }),
                Err(err) => {
                    log::error!("Plugin {}: {err}", file.display());
                    None
                }
            }
        })
        .collect();
    plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    plugins
}

pub fn find<'a>(plugins: &'a [Plugin], id: &str) -> Option<&'a Plugin> {
    plugins.iter().find(|p| p.manifest.id == id)
}

/// "./x" args are files in the plugin folder.
pub fn resolve(args: &[String], plugin_dir: &Path) -> Vec<String> {
    args.iter()
        .map(|a| match a.strip_prefix("./") {
            Some(rest) => plugin_dir.join(rest).to_string_lossy().into_owned(),
            None => a.clone(),
        })
        .collect()
}

/// Writes files (path -> bytes) as the plugin's folder, replacing an older version in one rename.
/// The manifest must be among them, parse, and carry this id.
pub fn install(root: &Path, id: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<Manifest, String> {
    if !valid_id(id) {
        return Err(format!("bad plugin id {id:?}"));
    }
    let manifest = files.get("plugin.json").ok_or("plugin.json missing")?;
    let manifest = Manifest::parse(&String::from_utf8_lossy(manifest))?;
    if manifest.id != id {
        return Err(format!("plugin.json says {}, not {id}", manifest.id));
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let tmp = root.join(format!(".{id}.new"));
    let old = root.join(format!(".{id}.old"));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&old);
    let write = || -> std::io::Result<()> {
        for (path, data) in files {
            if !safe_relative(path) {
                return Err(std::io::Error::other(format!("bad file path {path:?}")));
            }
            let f = tmp.join(path);
            std::fs::create_dir_all(f.parent().unwrap())?;
            std::fs::write(&f, data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e.to_string());
    }
    let dest = root.join(id);
    let swapped = (|| {
        if dest.exists() {
            std::fs::rename(&dest, &old)?;
        }
        std::fs::rename(&tmp, &dest)
    })();
    match swapped {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&old);
            Ok(manifest)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            if old.exists() && !dest.exists() {
                let _ = std::fs::rename(&old, &dest);
            }
            Err(e.to_string())
        }
    }
}

pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("bad plugin id {id:?}"));
    }
    std::fs::remove_dir_all(root.join(id)).map_err(|e| e.to_string())
}

/// First run of a plugin-aware version: the plugins folder doesn't exist yet, so every plugin
/// shipped inside the app is copied in. After that, what's installed is the user's choice.
pub fn install_bundled(bundled: &Path, root: &Path) {
    if root.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(root) {
        return log::error!("Cannot create {}: {e}", root.display());
    }
    for plugin in load(bundled) {
        let mut files = BTreeMap::new();
        if let Err(e) = read_tree(&plugin.dir, &plugin.dir, &mut files) {
            log::error!("Cannot read bundled plugin {}: {e}", plugin.manifest.id);
            continue;
        }
        match install(root, &plugin.manifest.id, &files) {
            Ok(m) => log::info!("Installed bundled plugin {} {}", m.id, m.version),
            Err(e) => log::error!("Cannot install bundled plugin {}: {e}", plugin.manifest.id),
        }
    }
}

fn read_tree(base: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        if p.is_dir() {
            read_tree(base, &p, files)?;
        } else {
            let rel = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            files.insert(rel, std::fs::read(&p)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system() -> Manifest {
        Manifest::parse(r#"{"id": "system", "name": "System", "version": "1.0.0",
            "fields": [{"id": "action", "label": "Action", "type": "select", "options": [
                {"value": "mute", "label": "Mute", "run": {"media": "mute"}},
                {"value": "lock", "label": "Lock Screen", "run": {"mac": {"keys": "ctrl+cmd+q"},
                    "windows": {"exec": ["rundll32.exe", "user32.dll,LockWorkStation"]}}},
                {"value": "winonly", "label": "W", "platforms": ["windows"], "run": {"exec": ["x"]}}]}]}"#).unwrap()
    }

    #[test]
    fn fills_placeholders() {
        let v = BTreeMap::from([("config".to_string(), "Home VPN".to_string()), ("x".into(), "1".into())]);
        assert_eq!(fill("{config}", &v), "Home VPN");
        assert_eq!(fill("VPN {config} {x}{missing}!", &v), "VPN Home VPN 1!");
        assert_eq!(fill("{ not a field } {", &v), "{ not a field } {");
    }

    #[test]
    fn exec_args_are_filled_one_by_one() {
        let m = Manifest::parse(r#"{"id": "vpn", "name": "Toggle VPN", "version": "1.0.0", "platforms": ["mac"],
            "fields": [{"id": "config", "label": "VPN", "type": "select"}, {"id": "mode", "label": "Mode", "type": "text"}],
            "run": {"exec": ["/bin/bash", "./toggle.sh", "{config}", "{mode}"]}}"#).unwrap();
        let mut a = Action::new("vpn", "a b; rm -rf ~");
        a.values.insert("mode".into(), "fast".into());
        assert_eq!(m.step(&a), Some(Step::Exec(vec!["/bin/bash".into(), "./toggle.sh".into(), "a b; rm -rf ~".into(), "fast".into()])));
        assert_eq!(m.step(&Action::new("vpn", "")), None); // nothing chosen yet
        let dir = Path::new("/p/vpn");
        assert_eq!(resolve(&["./toggle.sh".into(), "x".into()], dir), [dir.join("toggle.sh").to_string_lossy(), "x".into()]);
    }

    #[test]
    fn option_runs_and_platform_variants() {
        let m = system();
        assert_eq!(m.step(&Action::new("system", "mute")), Some(Step::Media("mute".into())));
        let lock = m.step(&Action::new("system", "lock")).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(lock, Step::Keys("ctrl+cmd+q".into()));
            assert_eq!(m.step(&Action::new("system", "winonly")), None);
        } else {
            assert!(matches!(lock, Step::Exec(_)));
        }
        assert_eq!(m.step(&Action::new("system", "unknown")), None);
        assert_eq!(m.describe(&Action::new("system", "lock")), "Lock Screen");
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = [
            r#"{"id": "Bad Id", "name": "x", "version": "1", "run": {"url": "x"}}"#,
            r#"{"id": "x", "name": "x", "version": "1"}"#,
            r#"{"id": "x", "name": "x", "version": "1", "run": {"url": "a", "app": "b"}}"#,
            r#"{"id": "x", "name": "x", "version": "1", "run": {"launch": "a"}}"#,
            r#"{"id": "x", "name": "x", "version": "1", "run": {"exec": []}}"#,
            r#"{"id": "x", "name": "x", "version": "1", "fields": [{"id": "f", "label": "F", "type": "slider"}], "run": {"url": "{f}"}}"#,
            r#"{"name": "x"}"#,
        ];
        for json in bad {
            assert!(Manifest::parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn checks_paths_and_ids() {
        assert!(safe_relative("plugin.json") && safe_relative("bin/x.sh"));
        for p in ["", "../x", "a/../../x", "/etc/passwd", "C:\\x", "a\\b", "./x"] {
            assert!(!safe_relative(p), "{p}");
        }
        assert!(valid_id("tunnelblick-vpn") && !valid_id("../x") && !valid_id("A") && !valid_id(""));
    }

    #[test]
    fn installs_replaces_loads_and_removes() {
        let root = std::env::temp_dir().join(format!("consoledeck-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let manifest = |v: &str| format!(r#"{{"id": "url", "name": "Open URL", "version": "{v}",
            "fields": [{{"id": "url", "label": "URL", "type": "url"}}], "run": {{"url": "{{url}}"}}}}"#).into_bytes();
        let files = BTreeMap::from([("plugin.json".to_string(), manifest("1.0.0")), ("bin/a.sh".into(), b"echo".to_vec())]);
        install(&root, "url", &files).unwrap();
        assert_eq!(load(&root)[0].manifest.version, "1.0.0");
        install(&root, "url", &BTreeMap::from([("plugin.json".to_string(), manifest("1.1.0"))])).unwrap();
        assert_eq!(load(&root)[0].manifest.version, "1.1.0");
        assert!(!root.join("url/bin/a.sh").exists()); // old files went with the old version
        // refused: wrong id, escaping path; the installed version is untouched
        assert!(install(&root, "app", &BTreeMap::from([("plugin.json".to_string(), manifest("2.0.0"))])).is_err());
        let evil = BTreeMap::from([("plugin.json".to_string(), manifest("2.0.0")), ("../evil".into(), vec![])]);
        assert!(install(&root, "url", &evil).is_err());
        assert!(!root.join("evil").exists() && !root.join(".url.new").exists());
        assert_eq!(load(&root)[0].manifest.version, "1.1.0");
        // a broken plugin is skipped, not fatal
        std::fs::create_dir_all(root.join("broken")).unwrap();
        std::fs::write(root.join("broken/plugin.json"), "{").unwrap();
        assert_eq!(load(&root).len(), 1);
        remove(&root, "url").unwrap();
        assert!(find(&load(&root), "url").is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The registry's plugins (app/plugins submodule) must all load here, so the app and
    /// tools/build_index.py agree on the format.
    #[test]
    fn registry_plugins_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/plugins");
        let count = std::fs::read_dir(&dir).expect("app/plugins submodule missing: git submodule update --init").count();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let json = std::fs::read_to_string(e.path().join("plugin.json")).unwrap();
            Manifest::parse(&json).unwrap_or_else(|err| panic!("{}: {err}", e.path().display()));
        }
        let here = load(&dir);
        assert!(here.len() >= count - 1 && find(&here, "system").is_some());
        let system = &find(&here, "system").unwrap().manifest;
        for o in &system.fields[0].options {
            assert!(system.step(&Action::new("system", &o.value)).is_some(), "system {} does nothing here", o.value);
        }
    }

    #[test]
    fn bundled_plugins_install_once() {
        let t = std::env::temp_dir().join(format!("consoledeck-bundled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (bundled, root) = (t.join("bundle"), t.join("plugins"));
        std::fs::create_dir_all(bundled.join("app")).unwrap();
        std::fs::write(bundled.join("app/plugin.json"), r#"{"id": "app", "name": "Open App", "version": "1.0.0",
            "fields": [{"id": "app", "label": "App", "type": "app"}], "run": {"app": "{app}"}}"#).unwrap();
        install_bundled(&bundled, &root);
        assert_eq!(load(&root).len(), 1);
        remove(&root, "app").unwrap();
        install_bundled(&bundled, &root); // folder exists now: removal sticks
        assert!(load(&root).is_empty());
        std::fs::remove_dir_all(&t).unwrap();
    }
}
