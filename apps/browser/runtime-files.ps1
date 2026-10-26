# 実行に必要な CEF のランタイムファイル。run.ps1(開発用の配置)と package.ps1(配布物)で共有する。
$CefRuntimeFiles = @(
    "libcef.dll", "chrome_elf.dll", "d3dcompiler_47.dll", "dxcompiler.dll", "vk_swiftshader.dll",
    "vk_swiftshader_icd.json", "vulkan-1.dll", "icudtl.dat", "resources.pak", "chrome_100_percent.pak",
    "chrome_200_percent.pak", "v8_context_snapshot.bin"
)

function Get-CefDir {
    $dir = Join-Path $env:USERPROFILE ".local\share\cef"
    if (-not (Test-Path (Join-Path $dir "libcef.dll"))) { throw "CEF runtime not found: $dir (run apps/browser/run.ps1 once)" }
    $dir
}
