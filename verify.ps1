# M0 検証: fmt / clippy / test / (任意) coverage / deny / memory bench
$ErrorActionPreference = 'Stop'
function Step($name, $block) { Write-Host "== $name"; & $block; if ($LASTEXITCODE -ne 0) { throw "$name failed" } }

Step 'fmt'    { cargo fmt --all -- --check }
Step 'clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Step 'test'   { cargo test --workspace }

if (Get-Command cargo-llvm-cov -ErrorAction SilentlyContinue) {
    Step 'coverage (>=80% lines)' { cargo llvm-cov --workspace --fail-under-lines 80 }
} else { Write-Warning 'cargo-llvm-cov 未導入のためカバレッジ検証をスキップ (cargo install cargo-llvm-cov)' }

if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Step 'deny' { cargo deny check }
} else { Write-Warning 'cargo-deny 未導入のためライセンス/脆弱性検査をスキップ (cargo install cargo-deny)' }

Step 'memory bench' { cargo run -q --release -p memory-bench -- --runs 3 }
Write-Host 'verify OK'
