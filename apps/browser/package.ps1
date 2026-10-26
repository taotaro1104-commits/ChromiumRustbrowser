# 配布用の zip を作る(署名は任意)。
#   powershell -NoProfile -ExecutionPolicy Bypass -File apps/browser/package.ps1 [-Sign] [-CertThumbprint <sha1>] [-SmokeTest]
#
# 構成: bootstrap.exe を `rustbrowser.exe` として置き、同名の `rustbrowser.dll`(このアプリ)を呼ばせる。
#   サンドボックスは bootstrap.exe が持つため、この構成でないとサンドボックスが無効になる。
# 署名: `-Sign` で signtool(Windows SDK)により exe と dll に SHA-256 で署名する。証明書の拇印は
#   `-CertThumbprint` か環境変数 RB_SIGN_THUMBPRINT で渡す(秘密鍵はこのリポジトリに置かない)。
# 検証: `-SmokeTest` は、できた zip を別の場所へ展開して起動し、ウィンドウが出るまでを確認する。
param(
    [switch]$Sign,
    [string]$CertThumbprint = $env:RB_SIGN_THUMBPRINT,
    [string]$TimestampUrl = "http://timestamp.digicert.com",
    [switch]$SmokeTest,
    [string]$OutDir = ""
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
. (Join-Path $PSScriptRoot "runtime-files.ps1")
$cef = Get-CefDir
$env:CEF_PATH = $cef

$version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $OutDir) { $OutDir = Join-Path $PSScriptRoot "dist" }
$name = "RustBrowser-$version-win-x64"
$stage = Join-Path $OutDir $name
$zip = Join-Path $OutDir "$name.zip"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "release build failed" }

Remove-Item -Recurse -Force $stage, $zip -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item (Join-Path $cef "bootstrap.exe") (Join-Path $stage "rustbrowser.exe")
Copy-Item (Join-Path $PSScriptRoot "target\release\rustbrowser.dll") $stage
foreach ($f in $CefRuntimeFiles) { Copy-Item (Join-Path $cef $f) $stage }
Copy-Item (Join-Path $cef "locales") (Join-Path $stage "locales") -Recurse
# CEF / Chromium のライセンス表記(再配布の条件)。
Copy-Item (Join-Path $cef "CREDITS.html") (Join-Path $stage "THIRD_PARTY_CREDITS.html")

if ($Sign) {
    if (-not $CertThumbprint) { throw "-Sign needs -CertThumbprint or RB_SIGN_THUMBPRINT" }
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signtool) { throw "signtool.exe not found (install the Windows SDK)" }
    foreach ($f in "rustbrowser.exe", "rustbrowser.dll") {
        & $signtool.FullName sign /sha1 $CertThumbprint /fd sha256 /tr $TimestampUrl /td sha256 (Join-Path $stage $f)
        if ($LASTEXITCODE -ne 0) { throw "signing $f failed" }
    }
}

Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
"{0}  {1:N0} MB  sha256={2}  signed={3}" -f $zip, ((Get-Item $zip).Length / 1MB), $hash, [bool]$Sign

if ($SmokeTest) {
    $run = Join-Path ([System.IO.Path]::GetTempPath()) ("rb-smoke-" + [guid]::NewGuid().ToString("N"))
    Expand-Archive -Path $zip -DestinationPath $run
    $env:RUSTBROWSER_PROFILE = Join-Path $run "profile"
    $p = Start-Process -FilePath (Join-Path $run "rustbrowser.exe") -PassThru
    try {
        $title = ""
        for ($i = 0; $i -lt 40 -and -not $title; $i++) {
            Start-Sleep -Milliseconds 500
            $p.Refresh()
            $title = $p.MainWindowTitle
        }
        if (-not $title) { throw "smoke test failed: no window appeared" }
        "smoke test OK: window title = '$title'"
    }
    finally {
        if (-not $p.HasExited) { [void]$p.CloseMainWindow(); [void]$p.WaitForExit(15000) }
        if (-not $p.HasExited) { & taskkill /F /T /PID $p.Id | Out-Null }
        Start-Sleep -Seconds 3
        Remove-Item -Recurse -Force $run -ErrorAction SilentlyContinue
    }
}
