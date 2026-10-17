//! Live buttons: a plugin's `live` command keeps running, and each line it prints is the button's text on the phone.

use crate::platform;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Live {
    running: Mutex<HashMap<Vec<String>, (Child, Arc<AtomicBool>)>>,
}

impl Live {
    /// Starts the commands in `wanted` that aren't running and stops the rest.
    /// A command keyed by its filled-in args runs once however many buttons show it.
    /// ponytail: one that exits by itself isn't restarted until it leaves `wanted` and comes back.
    pub fn sync(&self, wanted: Vec<(Vec<String>, PathBuf)>, on_line: Arc<dyn Fn(Vec<String>, String) + Send + Sync>) {
        let mut running = self.running.lock().unwrap();
        running.retain(|key, (child, on)| {
            let keep = wanted.iter().any(|(k, _)| k == key);
            if !keep {
                // the reader stops at the next line and closes the pipe, so the rest of a pipeline gets SIGPIPE
                on.store(false, Ordering::Relaxed);
                let _ = child.kill();
                let _ = child.wait();
            }
            keep
        });
        for (key, dir) in wanted {
            if running.contains_key(&key) {
                continue;
            }
            let args = crate::plugins::resolve(&key, &dir);
            let mut child = match platform::plugin_command(&args, &dir).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                Ok(c) => c,
                Err(e) => {
                    log::error!("{args:?} failed: {e}");
                    continue;
                }
            };
            let on = Arc::new(AtomicBool::new(true));
            let (stdout, flag, k, f) = (child.stdout.take().unwrap(), on.clone(), key.clone(), on_line.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if !flag.load(Ordering::Relaxed) {
                        break;
                    }
                    f(k.clone(), line.trim().into());
                }
            });
            running.insert(key, (child, on));
        }
    }
}

/// The text is cut to fit a tile; the chart numbers after it (tab-separated) are kept.
pub fn clip(line: &str) -> String {
    let (text, chart) = line.split_once('\t').map_or((line, None), |(t, c)| (t, Some(c)));
    let text: String = text.chars().take(20).collect();
    match chart {
        Some(c) => format!("{text}\t{}", c.chars().take(60).collect::<String>()),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_text_but_keeps_chart_numbers() {
        assert_eq!(clip("123456789012345678901234"), "12345678901234567890");
        assert_eq!(clip("↓128K ↑125K\t131072\t128000"), "↓128K ↑125K\t131072\t128000");
    }
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[cfg(unix)]
    #[test]
    fn streams_lines_and_stops() {
        let live = Live::default();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let cb: Arc<dyn Fn(Vec<String>, String) + Send + Sync> = Arc::new(move |k, l| {
            let _ = tx.lock().unwrap().send((k, l));
        });
        let key: Vec<String> = ["/bin/sh", "-c", "while true; do echo ' 42% '; sleep 0.1; done"].map(String::from).into();
        live.sync(vec![(key.clone(), std::env::temp_dir())], cb.clone());
        live.sync(vec![(key.clone(), std::env::temp_dir())], cb.clone()); // already running: not started twice
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), (key.clone(), "42%".into()));
        assert_eq!(live.running.lock().unwrap().len(), 1);
        live.sync(vec![], cb);
        assert!(live.running.lock().unwrap().is_empty());
    }
}
