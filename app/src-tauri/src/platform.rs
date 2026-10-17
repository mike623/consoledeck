//! Everything that differs between macOS and Windows: the app in front, input languages,
//! system commands and how apps and scripts are launched.

use serde::Serialize;
use std::process::Command;

/// The app in front, which picks the profile. `id` is the bundle ID on Mac, the exe name on Windows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FrontApp {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub pid: u32,
}

/// System actions: stored value, menu label. Order is the order shown in the editor.
pub const SYSTEM_ACTIONS: [(&str, &str); 13] = [
    ("lock", "Lock Screen"),
    ("sleep", "Sleep"),
    ("displaysleep", "Turn Display Off"),
    ("screensaver", "Start Screen Saver"),
    ("inputsource", "Next Input Language"),
    ("mute", "Mute / Unmute"),
    ("volumeup", "Volume Up"),
    ("volumedown", "Volume Down"),
    ("logout", "Log Out…"),
    ("restart", "Restart…"),
    ("shutdown", "Shut Down…"),
    ("editor", "Open ConsoleDeck Editor"),
    ("cheatsheet", "Show Cheat Sheet"),
];

fn spawn(args: &[&str]) {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    start(cmd, args);
}

fn start(mut cmd: Command, what: &[&str]) {
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    #[cfg(target_os = "macos")]
    // Apps launched from Finder get a bare PATH; scripts expect Homebrew tools.
    cmd.env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    if let Err(e) = cmd.spawn() {
        log::error!("{what:?} failed: {e}");
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};
    use objc2_app_kit::NSWorkspace;
    use std::ffi::c_void;

    pub fn front_app() -> Option<FrontApp> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let id = app.bundleIdentifier()?.to_string();
        let name = app.localizedName().map(|n| n.to_string()).unwrap_or_else(|| id.clone());
        Some(FrontApp { id, name, pid: app.processIdentifier() as u32 })
    }

    /// Log out / restart / shut down go through loginwindow, which shows macOS's own
    /// "Are you sure?" dialog, so a stray press can't lose work.
    pub fn system_command(id: &str) -> Option<Vec<&'static str>> {
        let osa = |script| vec!["/usr/bin/osascript", "-e", script];
        Some(match id {
            "sleep" => vec!["/usr/bin/pmset", "sleepnow"],
            "displaysleep" => vec!["/usr/bin/pmset", "displaysleepnow"],
            "screensaver" => vec!["/usr/bin/open", "-a", "ScreenSaverEngine"],
            "logout" => osa("tell application \"loginwindow\" to «event aevtlogo»"),
            "restart" => osa("tell application \"loginwindow\" to «event aevtrrst»"),
            "shutdown" => osa("tell application \"loginwindow\" to «event aevtrsdn»"),
            _ => return None,
        })
    }

    pub fn open_app(name: &str) {
        spawn(&["/usr/bin/open", "-a", name]);
    }

    pub fn run_script(script: &str) {
        spawn(&["/bin/bash", "-c", script]);
    }

    const TUNNELBLICK: &str = "/Applications/Tunnelblick.app";

    /// Tunnelblick's VPN configuration names, or None when it isn't installed.
    pub fn vpn_configs() -> Option<Vec<String>> {
        if !std::path::Path::new(TUNNELBLICK).exists() {
            return None;
        }
        let script = ["tell application \"Tunnelblick\" to set n to name of configurations",
            "set AppleScript's text item delimiters to linefeed", "n as text"];
        let out = Command::new("/usr/bin/osascript").args(script.iter().flat_map(|l| ["-e", l])).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
    }

    /// Tunnelblick copies it into its own store and asks the user how to install it.
    pub fn import_vpn(path: &std::path::Path) {
        spawn(&["/usr/bin/open", "-a", TUNNELBLICK, &path.to_string_lossy()]);
    }

    /// Connects the named configuration, or disconnects it when it's already up.
    pub fn toggle_vpn(name: &str) {
        let script = ["on run argv", "set n to item 1 of argv", "tell application \"Tunnelblick\"",
            "if (get state of first configuration where name = n) is \"CONNECTED\" then",
            "disconnect n", "else", "connect n", "end if", "end tell", "end run"];
        let mut args = vec!["/usr/bin/osascript"];
        args.extend(script.iter().flat_map(|l| ["-e", *l]));
        args.push(name);
        spawn(&args);
    }

    /// Picked .app bundle -> (bundle ID, name). The name is what `open -a` takes.
    pub fn app_identity(path: &std::path::Path) -> Option<(String, String)> {
        let out = Command::new("/usr/bin/defaults").arg("read").arg(path.join("Contents/Info")).arg("CFBundleIdentifier").output().ok()?;
        let id = String::from_utf8(out.stdout).ok()?.trim().to_string();
        let name = path.file_stem()?.to_string_lossy().into_owned();
        (!id.is_empty()).then_some((id, name))
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        static kTISPropertyInputSourceID: CFStringRef;
        static kTISPropertyLocalizedName: CFStringRef;
        static kTISPropertyInputSourceIsSelectCapable: CFStringRef;
        static kTISPropertyInputSourceCategory: CFStringRef;
        static kTISCategoryKeyboardInputSource: CFStringRef;
        fn TISCreateInputSourceList(properties: CFDictionaryRef, include_all: bool) -> CFArrayRef;
        fn TISGetInputSourceProperty(source: *const c_void, key: CFStringRef) -> *const c_void;
        fn TISSelectInputSource(source: *const c_void) -> i32;
        fn TISCopyCurrentKeyboardInputSource() -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    /// Keyboard layouts / input methods enabled in System Settings › Keyboard › Input Sources.
    /// TIS must be used on the main thread.
    fn enabled() -> Vec<CFType> {
        unsafe {
            let filter = CFDictionary::from_CFType_pairs(&[
                (CFString::wrap_under_get_rule(kTISPropertyInputSourceIsSelectCapable).as_CFType(), CFBoolean::true_value().as_CFType()),
                (CFString::wrap_under_get_rule(kTISPropertyInputSourceCategory).as_CFType(),
                 CFString::wrap_under_get_rule(kTISCategoryKeyboardInputSource).as_CFType()),
            ]);
            let list = TISCreateInputSourceList(filter.as_concrete_TypeRef(), false);
            if list.is_null() {
                return vec![];
            }
            CFArray::<CFType>::wrap_under_create_rule(list).iter().map(|s| s.clone()).collect()
        }
    }

    fn property(source: *const c_void, key: CFStringRef) -> String {
        let p = unsafe { TISGetInputSourceProperty(source, key) };
        if p.is_null() { String::new() } else { unsafe { CFString::wrap_under_get_rule(p as CFStringRef) }.to_string() }
    }

    fn source_id(source: *const c_void) -> String {
        property(source, unsafe { kTISPropertyInputSourceID })
    }

    /// Enabled input sources as (id, name). id is stored, e.g. "com.apple.keylayout.British".
    pub fn input_sources() -> Vec<(String, String)> {
        enabled().iter().map(|s| (source_id(s.as_CFTypeRef()), property(s.as_CFTypeRef(), unsafe { kTISPropertyLocalizedName }))).collect()
    }

    pub fn select_input_source(id: &str) {
        match enabled().iter().find(|s| source_id(s.as_CFTypeRef()) == id) {
            Some(s) => {
                let status = unsafe { TISSelectInputSource(s.as_CFTypeRef()) };
                if status != 0 {
                    log::error!("Input source switch failed: {status}");
                }
            }
            None => log::error!("Input source {id} not enabled"),
        }
    }

    /// Switch to the input source after the current one, wrapping around (like ⌃Space).
    pub fn next_input_source() {
        let sources = enabled();
        if sources.len() < 2 {
            return log::error!("Only one input source enabled");
        }
        let current = unsafe {
            let c = TISCopyCurrentKeyboardInputSource();
            let id = source_id(c);
            CFRelease(c);
            id
        };
        let i = sources.iter().position(|s| source_id(s.as_CFTypeRef()) == current).map_or(0, |i| i + 1);
        let status = unsafe { TISSelectInputSource(sources[i % sources.len()].as_CFTypeRef()) };
        if status != 0 {
            log::error!("Input source switch failed: {status}");
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
    use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST};

    fn exe_path(pid: u32) -> Option<std::path::PathBuf> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(h);
            ok.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]).into())
        }
    }

    pub fn front_app() -> Option<FrontApp> {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        let (id, name) = app_identity(&exe_path(pid)?)?;
        Some(FrontApp { id, name, pid })
    }

    /// No confirmation dialog on Windows, so restart and shut down wait 15s with a warning
    /// (`shutdown /a` cancels).
    pub fn system_command(id: &str) -> Option<Vec<&'static str>> {
        Some(match id {
            "sleep" => vec!["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"],
            "displaysleep" => vec!["powershell", "-NoProfile", "-Command",
                "(Add-Type '[DllImport(\"user32.dll\")]public static extern int SendMessage(int h,int m,int w,int l);' -Name W -PassThru)::SendMessage(0xffff,0x0112,0xF170,2)"],
            "screensaver" => vec!["scrnsave.scr", "/s"],
            "lock" => vec!["rundll32.exe", "user32.dll,LockWorkStation"],
            "logout" => vec!["shutdown", "/l"],
            "restart" => vec!["shutdown", "/r", "/t", "15"],
            "shutdown" => vec!["shutdown", "/s", "/t", "15"],
            _ => return None,
        })
    }

    /// cmd.exe doesn't understand Rust's \" argument escaping, so its command line is passed raw.
    fn cmd(line: String) {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.raw_arg("/C").raw_arg(&line);
        start(c, &["cmd", "/C", &line]);
    }

    /// A path from the picker, or anything `start` understands (notepad, ms-settings:).
    pub fn open_app(name: &str) {
        cmd(format!("start \"\" \"{name}\""));
    }

    pub fn run_script(script: &str) {
        cmd(script.to_string());
    }

    /// Tunnelblick is Mac only.
    pub fn vpn_configs() -> Option<Vec<String>> {
        None
    }

    pub fn import_vpn(_: &std::path::Path) {}

    /// exe path -> ("chrome.exe", "chrome"). Lowercase so profiles match however Windows reports it.
    pub fn app_identity(path: &std::path::Path) -> Option<(String, String)> {
        Some((path.file_name()?.to_string_lossy().to_lowercase(), path.file_stem()?.to_string_lossy().into_owned()))
    }

    fn layouts() -> Vec<HKL> {
        let mut list = [HKL::default(); 64];
        let n = unsafe { GetKeyboardLayoutList(Some(&mut list)) } as usize;
        list[..n.min(64)].to_vec()
    }

    fn layout_id(hkl: HKL) -> String {
        format!("{:08x}", hkl.0 as usize)
    }

    fn layout_name(hkl: HKL) -> String {
        let lang = (hkl.0 as usize & 0xffff) as u32;
        let mut locale = [0u16; 85];
        let mut name = [0u16; 256];
        unsafe {
            let n = LCIDToLocaleName(lang, Some(&mut locale), 0);
            if n == 0 {
                return layout_id(hkl);
            }
            let m = GetLocaleInfoEx(windows::core::PCWSTR(locale.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut name));
            if m == 0 { layout_id(hkl) } else { String::from_utf16_lossy(&name[..m as usize - 1]) }
        }
    }

    pub fn input_sources() -> Vec<(String, String)> {
        layouts().into_iter().map(|h| (layout_id(h), layout_name(h))).collect()
    }

    /// Windows switches the language per window, so ask the window in front to change.
    fn request(wparam: usize, hkl: isize) {
        if let Err(e) = unsafe { PostMessageW(Some(GetForegroundWindow()), WM_INPUTLANGCHANGEREQUEST, WPARAM(wparam), LPARAM(hkl)) } {
            log::error!("Input language switch failed: {e}");
        }
    }

    pub fn select_input_source(id: &str) {
        match layouts().into_iter().find(|h| layout_id(*h) == id) {
            Some(h) => request(0, h.0 as isize),
            None => log::error!("Input language {id} not enabled"),
        }
    }

    pub fn next_input_source() {
        request(0x0002, 1); // INPUTLANGCHANGE_FORWARD, HKL_NEXT
    }
}

pub use imp::*;

pub fn system_label(id: &str) -> Option<&'static str> {
    SYSTEM_ACTIONS.iter().find(|(i, _)| *i == id).map(|(_, l)| *l)
}

pub fn run_system_command(id: &str) {
    match system_command(id) {
        Some(args) => spawn(&args),
        None => log::error!("Unknown system action {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_actions_all_runnable() {
        // run inside the app (actions.rs) rather than as a command
        const IN_PROCESS: [&str; 7] = ["lock", "inputsource", "mute", "volumeup", "volumedown", "editor", "cheatsheet"];
        for (id, _) in SYSTEM_ACTIONS {
            assert!(IN_PROCESS.contains(&id) || system_command(id).is_some(), "no command for {id}");
        }
        assert_eq!(system_command("selfdestruct"), None);
        assert_eq!(system_label("restart"), Some("Restart…"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn reads_front_app_and_bundle_ids() {
        assert!(front_app().is_some_and(|a| !a.id.is_empty()));
        let (id, name) = app_identity(std::path::Path::new("/System/Applications/Calculator.app")).unwrap();
        assert_eq!((id.as_str(), name.as_str()), ("com.apple.calculator", "Calculator"));
    }
}
