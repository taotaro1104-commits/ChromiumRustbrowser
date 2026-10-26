//! `memory-bench [--runs N] [--baseline FILE] [--write-baseline]`
//! `memory-bench --attach PROCESS --scenario NAME [--samples N] [--interval-secs S] [--baseline FILE] [--write-baseline]`
//!
//! 前者は Rust 側(`rust_side`)を測る。後者は起動中のブラウザ(PROCESS に一致する全プロセスの
//! Private メモリ合計)を、準備済みの状態で測る。シナリオの状態(タブ数など)は呼び出し側で作っておく。
//! どちらも各シナリオの判定結果を表示し、予算超過または前回比 5% 超の増加があれば終了コード 1。
//! ベースライン(前回値)は `name=mb` 行のテキストファイル。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use memory_bench::{evaluate, measure_attached_mb, measure_rust_side_mb, Scenario, Verdict};

struct Args {
    runs: usize,
    baseline: Option<PathBuf>,
    write_baseline: bool,
    attach: Option<String>,
    scenario: Option<Scenario>,
    samples: usize,
    interval_secs: u64,
}

fn parse_count(flag: &str, value: Option<String>) -> Result<usize, String> {
    let v = value.ok_or(format!("{flag} needs a value"))?;
    let n: usize = v.parse().map_err(|_| format!("invalid {flag}: {v}"))?;
    if n == 0 || n > 99 {
        return Err(format!("{flag} must be 1..=99"));
    }
    Ok(n)
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        runs: 3,
        baseline: None,
        write_baseline: false,
        attach: None,
        scenario: None,
        samples: 3,
        interval_secs: 5,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--runs" => args.runs = parse_count("--runs", it.next())?,
            "--samples" => args.samples = parse_count("--samples", it.next())?,
            "--interval-secs" => {
                let v = it.next().ok_or("--interval-secs needs a value")?;
                args.interval_secs = v
                    .parse()
                    .map_err(|_| format!("invalid --interval-secs: {v}"))?;
            }
            "--baseline" => {
                args.baseline = Some(PathBuf::from(it.next().ok_or("--baseline needs a value")?))
            }
            "--write-baseline" => args.write_baseline = true,
            "--attach" => args.attach = Some(it.next().ok_or("--attach needs a process name")?),
            "--scenario" => {
                let v = it.next().ok_or("--scenario needs a name")?;
                args.scenario =
                    Some(Scenario::from_name(&v).ok_or_else(|| format!("unknown scenario: {v}"))?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if args.write_baseline && args.baseline.is_none() {
        return Err("--write-baseline requires --baseline FILE".into());
    }
    match (&args.attach, args.scenario) {
        (Some(_), None) => return Err("--attach requires --scenario NAME".into()),
        (Some(_), Some(Scenario::RustSide)) => {
            return Err("rust_side is measured without --attach".into())
        }
        (None, Some(_)) => return Err("--scenario is only used with --attach".into()),
        _ => {}
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

/// サンプルを集める。`--attach` では間隔を空けて、起動中のプロセスを繰り返し測る。
fn collect_samples(args: &Args) -> Result<Vec<f64>, String> {
    let runs = if args.attach.is_some() {
        args.samples
    } else {
        args.runs
    };
    let mut samples = Vec::with_capacity(runs);
    for i in 0..runs {
        let mb = match &args.attach {
            Some(name) => {
                let (mb, count) = measure_attached_mb(name)?;
                println!("  sample {}: {mb:.1} MB across {count} processes", i + 1);
                mb
            }
            None => measure_rust_side_mb()?,
        };
        samples.push(mb);
        if args.attach.is_some() && i + 1 < runs {
            std::thread::sleep(Duration::from_secs(args.interval_secs));
        }
    }
    Ok(samples)
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

    let scenarios: Vec<Scenario> = match args.scenario {
        Some(s) => vec![s],
        None => Scenario::ALL.to_vec(),
    };
    for scenario in scenarios {
        if args.attach.is_none() && !scenario.measurable_in_m0() {
            println!(
                "{:<12} SKIPPED  (budget {} MB; measure with --attach PROCESS --scenario {})",
                scenario.name(),
                scenario.budget_mb(),
                scenario.name()
            );
            continue;
        }
        let samples = match collect_samples(&args) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: measuring {}: {e}", scenario.name());
                return ExitCode::from(2);
            }
        };
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
            // 今回測っていないシナリオの前回値は残したまま、測ったものだけ更新する。
            let mut merged = read_baseline(path);
            for (k, v) in &results {
                merged.insert((*k).to_owned(), *v);
            }
            let body: String = merged
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
