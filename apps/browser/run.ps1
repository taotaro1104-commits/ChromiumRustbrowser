# P0 ブラウザのビルドと起動。初回は CEF の配布物(約 300MB)を取得する。
#   powershell -NoProfile -ExecutionPolicy Bypass -File apps/browser/run.ps1 [-Release] [-Url https://example.com/]
param([switch]$Release, [string]$Url = "")
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
. (Join-Path $PSScriptRoot "runtime-files.ps1")

$cef = Join-Path $env:USERPROFILE ".local\share\cef"
if (-not (Test-Path (Join-Path $cef "libcef.dll"))) {
    if (-not (Get-Command export-cef-dir -ErrorAction SilentlyContinue)) {
        cargo install export-cef-dir --locked
    }
    export-cef-dir --force $cef
}
$env:CEF_PATH = $cef

$profile = if ($Release) { "release" } else { "debug" }
if ($Release) { cargo build --release } else { cargo build }
if ($LASTEXITCODE -ne 0) { throw "build failed" }

# bootstrap.exe(サンドボックス付き)を `rustbrowser.exe` として置く。同名の DLL の RunWinMain が呼ばれる。
# 実行ファイルの隣に CEF のランタイムを置く。
$out = Join-Path $PSScriptRoot "target\$profile"
Copy-Item (Join-Path $cef "bootstrap.exe") (Join-Path $out "rustbrowser.exe") -Force
foreach ($f in $CefRuntimeFiles) { Copy-Item (Join-Path $cef $f) $out -Force }
Copy-Item (Join-Path $cef "locales") $out -Recurse -Force

$appArgs = @()
if ($Url -ne "") { $appArgs += "--url=$Url" }
& (Join-Path $out "rustbrowser.exe") @appArgs
