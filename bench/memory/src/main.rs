//! `memory-bench [--runs N] [--baseline FILE] [--write-baseline]`
//!
//! 各シナリオの判定結果を表示し、予算超過または前回比 5% 超の増加があれば終了コード 1。
//! ベースライン(前回値)は `name=mb` 行のテキストファイル。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use memory_bench::{evaluate, measure_rust_side_mb, Scenario, Verdict};

struct Args {
    runs: usize,
    baseline: Option<PathBuf>,
    write_baseline: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        runs: 3,
        baseline: None,
        write_baseline: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--runs" => {
                let v = it.next().ok_or("--runs needs a value")?;
                args.runs = v.parse().map_err(|_| format!("invalid --runs: {v}"))?;
                if args.runs == 0 || args.runs > 99 {
                    return Err("--runs must be 1..=99".into());
                }
            }
            "--baseline" => {
                args.baseline = Some(PathBuf::from(it.next().ok_or("--baseline needs a value")?))
            }
            "--write-baseline" => args.write_baseline = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if args.write_baseline && args.baseline.is_none() {
        return Err("--write-baseline requires --baseline FILE".into());
    }
    Ok(args)
}

fn read_baseline(path: &Path) -> BTreeMap<String, f64> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(k, v)| Some((k.trim().to_owned(), v.trim().parse().ok()?)))
        .collect()
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let baseline = args
        .baseline
        .as_deref()
        .map(read_baseline)
        .unwrap_or_default();
    let mut results = BTreeMap::new();
    let mut failed = false;

    for scenario in Scenario::ALL {
        if !scenario.measurable_in_m0() {
            println!(
                "{:<12} SKIPPED  (budget {} MB; needs a running CEF browser; not automated yet)",
                scenario.name(),
                scenario.budget_mb()
            );
            continue;
        }
        let mut samples = Vec::new();
        for _ in 0..args.runs {
            match measure_rust_side_mb() {
                Ok(mb) => samples.push(mb),
                Err(e) => {
                    eprintln!("error: measuring {}: {e}", scenario.name());
                    return ExitCode::from(2);
                }
            }
        }
        let verdict = evaluate(scenario, &samples, baseline.get(scenario.name()).copied());
        match verdict {
            Some(Verdict::Pass { mb }) => {
                println!(
                    "{:<12} PASS     {mb:.1} MB (budget {} MB)",
                    scenario.name(),
                    scenario.budget_mb()
                );
                results.insert(scenario.name(), mb);
            }
            Some(Verdict::OverBudget { mb, budget_mb }) => {
                println!(
                    "{:<12} FAIL     {mb:.1} MB exceeds budget {budget_mb} MB",
                    scenario.name()
                );
                failed = true;
            }
            Some(Verdict::Regression { mb, previous_mb }) => {
                println!(
                    "{:<12} FAIL     {mb:.1} MB is >5% above previous {previous_mb:.1} MB",
                    scenario.name()
                );
                failed = true;
            }
            None => {
                eprintln!("error: no samples for {}", scenario.name());
                return ExitCode::from(2);
            }
        }
    }

    if args.write_baseline && !failed {
        if let Some(path) = &args.baseline {
            let body: String = results
                .iter()
                .map(|(k, v)| format!("{k}={v:.3}\n"))
                .collect();
            if let Err(e) = std::fs::write(path, body) {
                eprintln!("error: writing baseline {path:?}: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
