//! Everything that differs between macOS and Windows: the app in front, input languages
//! and how apps, scripts and plugin commands are launched.

use serde::Serialize;
use std::process::{Command, Stdio};

/// The app in front, which picks the profile. `id` is the bundle ID on Mac, the exe name on Windows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FrontApp {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub pid: u32,
}

fn spawn(args: &[&str]) {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    start(cmd, args);
}

/// A plugin command, run in its folder; CONSOLEDECK_DIR is the folder deck.json is in.
pub fn plugin_command(args: &[String], dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..]);
    prepare(&mut cmd);
    cmd.current_dir(dir).env("CONSOLEDECK_DIR", crate::config::Config::path().parent().unwrap());
    cmd
}

pub fn run_plugin(args: &[String], dir: &std::path::Path) {
    let what: Vec<&str> = args.iter().map(String::as_str).collect();
    if let Err(e) = plugin_command(args, dir).spawn() {
        log::error!("{what:?} failed: {e}");
    }
}

fn start(mut cmd: Command, what: &[&str]) {
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    prepare(&mut cmd);
    // Waited on in a thread so a failing script says why in the log (its stderr), not just nothing.
    let child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn();
    let what = what.join(" ");
    match child {
        Ok(child) => {
            std::thread::spawn(move || match child.wait_with_output() {
                Ok(out) if !out.status.success() => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let tail: Vec<&str> = err.trim().lines().rev().take(5).collect(); // ponytail: last 5 lines are enough to see why
                    let tail: Vec<&str> = tail.into_iter().rev().collect();
                    log::error!("{what} failed ({}): {}", out.status, tail.join(" | "));
                }
                Ok(_) => {}
                Err(e) => log::error!("{what}: {e}"),
            });
        }
        Err(e) => log::error!("{what} failed: {e}"),
    }
}

fn prepare(cmd: &mut Command) {
    #[cfg(target_os = "macos")]
    // Apps launched from Finder get a bare PATH; scripts expect Homebrew tools and per-user ones (uv, pipx, cargo).
    {
        let home = dirs::home_dir().unwrap_or_default();
        let home = home.display();
        cmd.env("PATH", format!("{home}/.local/bin:{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
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

    /// System Settings > Appearance is Dark (Auto included: it sets the same key).
    pub fn dark_mode() -> bool {
        let style = objc2_foundation::NSUserDefaults::standardUserDefaults()
            .stringForKey(&objc2_foundation::NSString::from_str("AppleInterfaceStyle"));
        style.is_some_and(|s| s.to_string() == "Dark")
    }

    pub fn front_app() -> Option<FrontApp> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let id = app.bundleIdentifier()?.to_string();
        let name = app.localizedName().map(|n| n.to_string()).unwrap_or_else(|| id.clone());
        Some(FrontApp { id, name, pid: app.processIdentifier() as u32 })
    }

    pub fn open_app(name: &str) {
        spawn(&["/usr/bin/open", "-a", name]);
    }

    pub fn run_script(script: &str) {
        spawn(&["/bin/bash", "-c", script]);
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

    /// Settings > Personalization > Colors: apps in dark mode.
    pub fn dark_mode() -> bool {
        use windows::core::w;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut light = 1u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let read = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD, None, Some(&mut light as *mut u32 as *mut _), Some(&mut size as *mut u32))
        };
        read.is_ok() && light == 0
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn dark_mode_matches_system_settings() {
        let out = Command::new("/usr/bin/defaults").args(["read", "-g", "AppleInterfaceStyle"]).output().unwrap();
        assert_eq!(dark_mode(), String::from_utf8_lossy(&out.stdout).trim() == "Dark");
    }

    #[test]
    #[cfg(unix)]
    fn plugin_commands_run_in_their_folder() {
        let dir = std::env::temp_dir().join(format!("consoledeck-exec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.sh"), "echo \"$1|$(basename \"$PWD\")|$CONSOLEDECK_DIR\"").unwrap();
        let args = crate::plugins::resolve(&["/bin/bash".into(), "./x.sh".into(), "a b".into()], &dir);
        let out = plugin_command(&args, &dir).output().unwrap();
        let config_dir = crate::config::Config::path().parent().unwrap().display().to_string();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("a b|{}|{config_dir}", dir.file_name().unwrap().to_string_lossy()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn reads_front_app_and_bundle_ids() {
        assert!(front_app().is_some_and(|a| !a.id.is_empty()));
        let (id, name) = app_identity(std::path::Path::new("/System/Applications/Calculator.app")).unwrap();
        assert_eq!((id.as_str(), name.as_str()), ("com.apple.calculator", "Calculator"));
    }
}
