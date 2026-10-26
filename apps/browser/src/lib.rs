//! Rust ブラウザ P0 プロトタイプ(CEF 経由)。docs/P0-plan.md の P0-3/P0-4。
//! ロジックは `shell-core`、CEF とのつなぎ込みだけをこのライブラリに置く。

mod app;
mod ctx;
mod handlers;
mod host;
mod page_state;
mod profile;
mod start_page;
mod tab_layout;
mod ui;

use std::ffi::c_void;

use cef::*;

/// CEF の `bootstrap.exe` から呼ばれるエントリポイント。
///
/// bootstrap.exe はサンドボックス用の静的ライブラリを持つ実行ファイルで、同名の DLL(`rustbrowser.dll`)
/// の `RunWinMain` を呼ぶ。子プロセス(レンダラー等)も同じ経路で起動するので、全プロセスがサンドボックスの
/// 初期化を受ける。このクレートで `unsafe` を使うのは、この C ABI の境界だけ。
#[unsafe(no_mangle)]
pub extern "system" fn RunWinMain(
    _instance: *mut c_void,
    _cmd_line: *mut u16,
    _show: i32,
    sandbox_info: *mut c_void,
    _version_info: *mut c_void,
) -> i32 {
    match run(sandbox_info) {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("browser: {msg}");
            1
        }
    }
}

fn run(sandbox_info: *mut c_void) -> Result<(), &'static str> {
    // CEF の API バージョンを初期化する。
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = args::Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return Err("failed to parse command line");
    };

    let switch = CefString::from("type");
    let is_browser_process = cmd_line.has_switch(Some(&switch)) != 1;
    let ret = execute_process(Some(args.as_main_args()), None, sandbox_info.cast());
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
        // サンドボックスは常に有効。無効化する設定は持たない。
        no_sandbox: 0,
        root_cache_path: cache.clone(),
        cache_path: cache,
        ..Default::default()
    };
    let mut app = app::BrowserApp::new();
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        sandbox_info.cast(),
    ) != 1
    {
        return Err("cef initialize failed");
    }
    run_message_loop();
    shutdown();
    Ok(())
}
