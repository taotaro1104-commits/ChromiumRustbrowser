# CEF 版ブラウザ(release)のメモリ 3 シナリオを、起動から計測・判定まで自動で行う。
#   powershell -NoProfile -ExecutionPolicy Bypass -File bench/memory/run-cef-bench.ps1 [-Scenarios startup,single_page,tabs_20] [-WriteBaseline] [-Sites real|local]
#
# 前提: `apps/browser/run.ps1 -Release` を一度実行して、target/release に実行ファイルと CEF のランタイムが揃っていること。
# シナリオごとに、空のプロファイルで起動 → 読み込みが落ち着くまで待つ → `memory-bench --attach` → 終了、を行う。
# 終了コード: 0=全通過、1=予算超過/前回比超過、2=準備や計測の失敗。
param(
    [string[]]$Scenarios = @("startup", "single_page", "tabs_20"),
    [switch]$WriteBaseline,
    # real: 実サイト 19 枚(予算の根拠と同じ。ネットワークが要る)。local: 127.0.0.N のローカルページ(軽く、決定的)。
    [ValidateSet("real", "local")][string]$Sites = "real",
    # 空きメモリがこれを下回るなら計測しない(OS にプロセスを止められて結果が無効になるため)。
    [double]$MinFreeGB = 4.0,
    [int]$SamplesPerScenario = 3
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$exe = Join-Path $root "apps\browser\target\release\rustbrowser.exe"
$baseline = Join-Path $PSScriptRoot "baseline.txt"
$bench = "cargo"

$realSites = "example.com", "example.org", "example.net", "wikipedia.org", "github.com", "rust-lang.org",
    "duckduckgo.com", "mozilla.org", "python.org", "w3.org", "iana.org", "openstreetmap.org", "archive.org",
    "gnu.org", "kernel.org", "debian.org", "apache.org", "eff.org", "ietf.org"

function Get-FreeGB { (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB }

function Get-Urls([string]$scenario) {
    switch ($scenario) {
        "startup" { @() }
        "single_page" { @("https://example.com/") }
        "tabs_20" {
            if ($Sites -eq "real") { $realSites | ForEach-Object { "https://$_/" } }
            else { 2..20 | ForEach-Object { "http://127.0.0.$_`:8765/index.html" } }
        }
    }
}

# 起動後、プロセス数と Private メモリが落ち着くまで待つ(最大 $maxSec 秒)。
function Wait-Settled([string]$name, [int]$minSec, [int]$maxSec) {
    Start-Sleep -Seconds $minSec
    $prev = -1; $stable = 0; $waited = $minSec
    while ($waited -lt $maxSec -and $stable -lt 2) {
        Start-Sleep -Seconds 5; $waited += 5
        $p = Get-Process $name -ErrorAction SilentlyContinue
        $mb = ($p | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
        if ($prev -gt 0 -and [math]::Abs($mb - $prev) / $prev -lt 0.02) { $stable++ } else { $stable = 0 }
        $prev = $mb
    }
}

if (-not (Test-Path $exe)) { Write-Error "release build not found: $exe (run apps/browser/run.ps1 -Release first)"; exit 2 }
if ((Get-FreeGB) -lt $MinFreeGB) {
    Write-Error ("free memory {0:N1} GB < {1} GB; close other apps and retry" -f (Get-FreeGB), $MinFreeGB); exit 2
}

$worst = 0
foreach ($scenario in $Scenarios) {
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("rb-bench-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $env:RUSTBROWSER_PROFILE = $tmp
    $urls = @(Get-Urls $scenario)
    $appArgs = @()
    if ($urls.Count -gt 0) {
        $list = Join-Path $tmp "urls.txt"
        Set-Content -Path $list -Value $urls -Encoding utf8
        $appArgs += "--urls-file=$list"
    }
    Write-Host "== $scenario ($($urls.Count) urls)"
    $startArgs = @{ FilePath = $exe; PassThru = $true }
    if ($appArgs.Count -gt 0) { $startArgs.ArgumentList = $appArgs }
    $proc = Start-Process @startArgs
    try {
        Wait-Settled "rustbrowser" ($(if ($urls.Count -gt 5) { 40 } else { 15 })) 120
        $cargoArgs = @("run", "-q", "-p", "memory-bench", "--", "--attach", "rustbrowser", "--scenario", $scenario,
            "--samples", $SamplesPerScenario, "--baseline", $baseline)
        if ($WriteBaseline) { $cargoArgs += "--write-baseline" }
        Push-Location $root
        & $bench @cargoArgs
        $code = $LASTEXITCODE
        Pop-Location
        if ($code -gt $worst) { $worst = $code }
    }
    finally {
        # まずウィンドウを閉じて正常終了を促し、残れば自分が起動した木だけを止める。
        if (-not $proc.HasExited) { [void]$proc.CloseMainWindow(); [void]$proc.WaitForExit(40000) }
        if (-not $proc.HasExited) { & taskkill /F /T /PID $proc.Id 2>$null | Out-Null }
        Start-Sleep -Seconds 5
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}
exit $worst
