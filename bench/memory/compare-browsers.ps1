# Rust Browser(CEF)と Chrome / Edge を、同じシナリオ・同じ測り方でメモリ比較する。
#   powershell -NoProfile -ExecutionPolicy Bypass -File bench/memory/compare-browsers.ps1 [-Browsers rustbrowser,chrome,edge] [-Scenarios startup,single_page,tabs_20]
#
# 各ブラウザを **空の一時プロファイル**で新規に起動し(利用者の普段のブラウザには触れない)、落ち着くまで待って、
# 起動したプロセスの **子孫すべて** の Private バイトを 3 回測って中央値を取る。測り終えたら、その木だけを終了する。
# シナリオ(自動計測の状態に揃える):
#   startup     : 起動直後。Rust Browser = スタートページ、Chrome/Edge = about:blank(ブラウザ自身の新規タブページを避ける)
#   single_page : https://example.com/ だけ
#   tabs_20     : 実サイト 19 枚を同時に開く(run-cef-bench.ps1 と同じ一覧)
# 注意: 余計な常駐・拡張・同期・セーフブラウジング等はブラウザの既定のまま(ブラウザ本来の構成での比較)。
param(
    [string[]]$Browsers = @("rustbrowser", "chrome", "edge"),
    [string[]]$Scenarios = @("startup", "single_page", "tabs_20"),
    [double]$MinFreeGB = 3.5,
    [int]$Samples = 3
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")

$exes = @{
    rustbrowser = Join-Path $root "apps\browser\target\release\rustbrowser.exe"
    chrome      = "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe"
    edge        = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
}
$sites = "example.com", "example.org", "example.net", "wikipedia.org", "github.com", "rust-lang.org",
    "duckduckgo.com", "mozilla.org", "python.org", "w3.org", "iana.org", "openstreetmap.org", "archive.org",
    "gnu.org", "kernel.org", "debian.org", "apache.org", "eff.org", "ietf.org"

function Get-FreeGB { (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB }

# $rootPid の子孫すべての Private バイト(MB)とプロセス数。
function Get-TreeStats([int]$rootPid) {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
    $ids = New-Object System.Collections.Generic.HashSet[int]
    [void]$ids.Add($rootPid)
    do {
        $before = $ids.Count
        foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId)) { [void]$ids.Add([int]$p.ProcessId) } }
    } while ($ids.Count -ne $before)
    $sum = 0; $n = 0
    foreach ($id in $ids) {
        $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($proc) { $sum += $proc.PrivateMemorySize64; $n++ }
    }
    [pscustomobject]@{ MB = $sum / 1MB; Procs = $n }
}

function Get-Urls([string]$scenario, [string]$browser) {
    switch ($scenario) {
        "startup" { if ($browser -eq "rustbrowser") { @() } else { @("about:blank") } }
        "single_page" { @("https://example.com/") }
        "tabs_20" { $sites | ForEach-Object { "https://$_/" } }
    }
}

function Wait-Settled([int]$rootPid, [int]$minSec, [int]$maxSec) {
    Start-Sleep -Seconds $minSec
    $prev = -1; $stable = 0; $waited = $minSec
    while ($waited -lt $maxSec -and $stable -lt 2) {
        Start-Sleep -Seconds 5; $waited += 5
        $mb = (Get-TreeStats $rootPid).MB
        if ($prev -gt 0 -and [math]::Abs($mb - $prev) / $prev -lt 0.02) { $stable++ } else { $stable = 0 }
        $prev = $mb
    }
}

function Get-Median([double[]]$v) {
    $s = $v | Sort-Object
    $s[[int][math]::Floor($s.Count / 2)]
}

$results = @()
foreach ($browser in $Browsers) {
    $exe = $exes[$browser]
    if (-not (Test-Path $exe)) { Write-Warning "$browser not found: $exe"; continue }
    $version = (Get-Item $exe).VersionInfo.ProductVersion
    foreach ($scenario in $Scenarios) {
        if ((Get-FreeGB) -lt $MinFreeGB) {
            Write-Error ("free memory {0:N1} GB < {1} GB; stopping" -f (Get-FreeGB), $MinFreeGB); exit 2
        }
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("rb-cmp-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $urls = @(Get-Urls $scenario $browser)
        $startArgs = @{ FilePath = $exe; PassThru = $true }
        if ($browser -eq "rustbrowser") {
            $env:RUSTBROWSER_PROFILE = $tmp
            if ($urls.Count -gt 0) {
                $list = Join-Path $tmp "urls.txt"
                Set-Content -Path $list -Value $urls -Encoding utf8
                $startArgs.ArgumentList = @("--urls-file=$list")
            }
        }
        else {
            $startArgs.ArgumentList = @("--user-data-dir=$tmp", "--no-first-run", "--no-default-browser-check") + $urls
        }
        Write-Host "== $browser $version / $scenario ($($urls.Count) urls)"
        $proc = Start-Process @startArgs
        try {
            Wait-Settled $proc.Id ($(if ($urls.Count -gt 5) { 45 } else { 20 })) 150
            $vals = @(); $procs = 0
            for ($i = 0; $i -lt $Samples; $i++) {
                $st = Get-TreeStats $proc.Id
                $vals += $st.MB; $procs = $st.Procs
                if ($i + 1 -lt $Samples) { Start-Sleep -Seconds 5 }
            }
            $med = Get-Median $vals
            Write-Host ("   {0:N0} MB across {1} processes  (samples: {2})" -f $med, $procs, (($vals | ForEach-Object { "{0:N0}" -f $_ }) -join ", "))
            $results += [pscustomobject]@{ Browser = $browser; Version = $version; Scenario = $scenario; MB = [math]::Round($med); Procs = $procs }
        }
        finally {
            & taskkill /F /T /PID $proc.Id 2>$null | Out-Null
            Start-Sleep -Seconds 8
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }
    }
}
Write-Host ""
Write-Host "== summary (Private MB, median of $Samples)"
$results | Format-Table Browser, Version, Scenario, MB, Procs -AutoSize | Out-String -Width 200 | Write-Host
$results | ConvertTo-Json | Set-Content -Path (Join-Path ([System.IO.Path]::GetTempPath()) "rb-compare-result.json") -Encoding utf8
