# Builds the ModNest installer with the native (GPUI) app inside.
# It is the same NSIS installer as before (same install folder, shortcuts and update signature);
# only the program in it is the native build instead of the WebView one.
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-native.ps1
#
# Needs: the GNU Rust toolchain, Node (npx tauri), the fxc stand-in (tools\fxc-shim), and
# the signing key in modnest.key (repo root) or %USERPROFILE%\.tauri\modnest.key.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$cargoBin = Join-Path (Join-Path $env:USERPROFILE '.cargo') 'bin'
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) { $env:PATH = "$cargoBin;$env:PATH" }
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $env:USERPROFILE '.modnest-target' }
if (-not $env:GPUI_FXC_PATH) { $env:GPUI_FXC_PATH = Join-Path $env:USERPROFILE '.modnest-tools\fxc\fxc.exe' }
$key = @((Join-Path $root 'modnest.key'), (Join-Path $env:USERPROFILE '.tauri\modnest.key')) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $key) { throw 'No signing key (modnest.key) found.' }
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''

cargo build --release -p modnest-native
if ($LASTEXITCODE) { throw 'native build failed' }

$release = Join-Path $env:CARGO_TARGET_DIR 'release'
$native = Join-Path $release 'modnest-native.exe'
$main = Join-Path $release 'modnest.exe'
# Tauri builds its own modnest.exe first; this hook puts the native one in its place before bundling.
$hook = "powershell -NoProfile -Command Copy-Item -Force '$native' '$main'"
$config = @{
  build = @{ beforeBundleCommand = $hook }
  bundle = @{ windows = @{ webviewInstallMode = @{ type = 'skip' } } }
} | ConvertTo-Json -Depth 5 -Compress
$cfg = Join-Path $env:TEMP 'modnest-native-bundle.json'
[IO.File]::WriteAllText($cfg, $config)

npx tauri build --config $cfg
if ($LASTEXITCODE) { throw 'packaging failed' }
Get-ChildItem (Join-Path $release 'bundle\nsis') | Select-Object Name, Length
