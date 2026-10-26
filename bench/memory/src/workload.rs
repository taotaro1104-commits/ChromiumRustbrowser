use std::path::PathBuf;

use browser_core::{Action, Store};
use ffi_traits::mock::MockHost;
use ffi_traits::MemoryPressure;
use storage::Db;
use sysinfo::{ProcessesToUpdate, System};

const TABS: u64 = 20;
const HISTORY_ROWS: u64 = 10_000;
const QUERIES: u64 = 200;
const MB: f64 = 1024.0 * 1024.0;

/// 代表的な負荷(20タブの開閉・凍結・破棄、履歴1万件の投入と検索)を流し、
/// 自プロセスのワーキングセット(MB)を返す。履歴は実ファイルの DB に置く
/// (メモリ DB だとデータ全量が常駐してしまい実態と乖離するため)。
pub fn measure_rust_side_mb() -> Result<f64, String> {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {dir:?}: {e}"))?;
    let result = run_workload(&dir.join("bench.sqlite"));
    // 後始末の失敗は計測結果を無効にしない。
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    current_process_mb()
}

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rb-memory-bench-{}", std::process::id()))
}

fn run_workload(db_path: &std::path::Path) -> Result<(), String> {
    let mut store = Store::new(MockHost::new());
    for i in 0..TABS {
        store
            .dispatch(Action::OpenTab {
                url: format!("https://site{i}.example/"),
                now_ms: i * 1000,
            })
            .map_err(|e| e.to_string())?;
    }
    for step in 1..=6u64 {
        store
            .dispatch(Action::Tick {
                now_ms: step * 10 * 60_000,
            })
            .map_err(|e| e.to_string())?;
    }
    store
        .dispatch(Action::MemoryPressure {
            level: MemoryPressure::Critical,
        })
        .map_err(|e| e.to_string())?;

    let db = Db::open(db_path).map_err(|e| e.to_string())?;
    for i in 0..HISTORY_ROWS {
        db.record_visit(
            &format!("https://host{}.example/page/{i}", i % 500),
            &format!("Page {i} title"),
            i,
        )
        .map_err(|e| e.to_string())?;
    }
    for q in 0..QUERIES {
        db.search_history(&format!("page {}", q % 100), 10)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn current_process_mb() -> Result<f64, String> {
    let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let p = sys.process(pid).ok_or("current process not found")?;
    Ok(p.memory() as f64 / MB)
}
