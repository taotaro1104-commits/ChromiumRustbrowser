//! Rust ブラウザ P0 プロトタイプ(CEF 経由)。docs/P0-plan.md の P0-3/P0-4。
//! ロジックは `shell-core`、CEF とのつなぎ込みだけをこのバイナリに置く。

mod app;
mod ctx;
mod handlers;
mod host;
mod profile;
mod tab_layout;
mod ui;

use cef::*;

fn main() -> Result<(), &'static str> {
    // CEF の API バージョンを初期化する。
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = args::Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return Err("failed to parse command line");
    };

    let switch = CefString::from("type");
    let is_browser_process = cmd_line.has_switch(Some(&switch)) != 1;
    let ret = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    if !is_browser_process {
        // レンダラー等の子プロセス。CEF を初期化せずに終了する。
        return if ret >= 0 {
            Ok(())
        } else {
            Err("cannot execute non-browser process")
        };
    }
    if ret != -1 {
        return Err("cannot execute browser process");
    }

    let profile_dir = profile::profile_dir();
    let cache = CefString::from(profile_dir.join("cef-cache").to_string_lossy().as_ref());
    let settings = Settings {
        no_sandbox: 1,
        root_cache_path: cache.clone(),
        cache_path: cache,
        ..Default::default()
    };
    let mut app = app::BrowserApp::new();
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("cef initialize failed");
    }
    run_message_loop();
    shutdown();
    Ok(())
}
