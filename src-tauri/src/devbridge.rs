use std::io::{BufRead, BufReader};

use tauri::{Emitter, Manager};

pub fn spawn(app: &tauri::AppHandle) {
    let Ok(path) = std::env::var("THELEMAIL_DESKTOP_EVAL_FIFO") else {
        return;
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            let Ok(file) = std::fs::File::open(&path) else {
                return;
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let script = line;
                if script.trim().is_empty() {
                    continue;
                }
                if let Some(window) = handle.get_webview_window("main") {
                    let wrapped = format!(
                        "(async () => {{ const say = (kind, message, detail) => window.__TAURI_INTERNALS__.invoke('ui_diagnostic', {{ report: {{ kind, message, detail: detail ?? null }} }}); try {{ const r = await (async () => {{ {script} }})(); say('eval', String(r)); }} catch (e) {{ say('eval-error', String(e), e && e.stack ? e.stack : null); }} }})()"
                    );
                    let _ = window.eval(&wrapped);
                }
                let _ = handle.emit("devbridge", ());
            }
        }
    });
}
