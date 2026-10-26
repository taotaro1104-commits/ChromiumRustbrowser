# P0 ブラウザのビルドと起動。初回は CEF の配布物(約 300MB)を取得する。
#   powershell -NoProfile -ExecutionPolicy Bypass -File apps/browser/run.ps1 [-Release] [-Url https://example.com/]
param([switch]$Release, [string]$Url = "")
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

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
$files = "libcef.dll", "chrome_elf.dll", "d3dcompiler_47.dll", "dxcompiler.dll", "vk_swiftshader.dll",
    "vk_swiftshader_icd.json", "vulkan-1.dll", "icudtl.dat", "resources.pak", "chrome_100_percent.pak",
    "chrome_200_percent.pak", "v8_context_snapshot.bin"
foreach ($f in $files) { Copy-Item (Join-Path $cef $f) $out -Force }
Copy-Item (Join-Path $cef "locales") $out -Recurse -Force

$args = @()
if ($Url -ne "") { $args += "--url=$Url" }
& (Join-Path $out "rustbrowser.exe") @args
