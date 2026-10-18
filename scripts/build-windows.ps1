# Windows release build: the per-user NSIS installer (x64) in dist\release\<version>\.
# Run it on Windows (Visual Studio Build Tools with the C++ workload, Strawberry Perl, rustup, Node 26,
# pnpm 12, git); FreeRDP is built the first time (scripts/build-freerdp.ps1).
#
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
#   … -NoUpdater     without updater signatures even if the key is found
#   … -AllowDirty    build with uncommitted changes (test builds only)
#
#   → RemoteDesktop_<version>_windows_x64-setup.exe (+ .sig for the updater when the key is found)
#
# Updater signatures: with $env:TAURI_SIGNING_PRIVATE_KEY set, or the key file at
# $env:TAURI_SIGNING_PRIVATE_KEY_PATH (default ~\.tauri\remotedesktop.key); the key's password comes
# from $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD or is asked for once.
param(
    [switch]$NoUpdater,
    [switch]$AllowDirty
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

function Step($message) { Write-Host "`n==> $message" -ForegroundColor White }
function Die($message) { Write-Host "error: $message" -ForegroundColor Red; exit 1 }
# Runs a native command and stops on a non-zero exit code (PowerShell 5 doesn't by itself).
function Run {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE -ne 0) { Die "$($args -join ' ') failed (exit code $LASTEXITCODE)" }
}

# A shell opened before rustup was installed lacks its folder on PATH; use it where rustup puts it.
$cargoBin = Join-Path $HOME '.cargo\bin'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $cargoBin 'cargo.exe'))) {
    $env:Path = "$cargoBin;$env:Path"
}

# The MSVC toolchain, checked up front and loaded into this shell: Rust links with MSVC's link.exe against
# the Windows SDK, and with the environment loaded no other link.exe on PATH (Git's) gets in the way.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) {
    Die "Visual Studio Build Tools not found: install them with the 'Desktop development with C++' workload"
}
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) {
    Die "Visual Studio has no C++ tools: in the Visual Studio Installer add 'Desktop development with C++' (it brings the Windows SDK too)"
}
if (-not (Test-Path (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\Lib'))) {
    Die "the Windows SDK is missing: in the Visual Studio Installer add a Windows 11 (or 10) SDK to the C++ tools"
}
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
}

foreach ($tool in 'cargo', 'rustup', 'node', 'pnpm', 'git', 'perl', 'link') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Die "$tool not found: install it and open a new PowerShell window"
    }
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"$' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { Die 'no [workspace.package] version in Cargo.toml' }
$commit = (git rev-parse HEAD).Trim()
$out = "dist\release\$version"
$target = 'x86_64-pc-windows-msvc'

if (-not $AllowDirty -and (git status --porcelain)) {
    Die 'uncommitted changes; commit them first (or -AllowDirty for a test build)'
}

# --- updater key ---
$sign = $false
if (-not $NoUpdater) {
    $keyFile = if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH) { $env:TAURI_SIGNING_PRIVATE_KEY_PATH } else { Join-Path $HOME '.tauri\remotedesktop.key' }
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and (Test-Path $keyFile)) {
        $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw $keyFile).Trim()
    }
    if ($env:TAURI_SIGNING_PRIVATE_KEY) {
        $sign = $true
        if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
            $secure = Read-Host 'Updater key password' -AsSecureString
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
                [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
        }
    } else {
        Write-Host "note: no updater key ($keyFile); building without updater signatures (.sig)." -ForegroundColor Yellow
    }
}
$tauriArgs = @('--target', $target, '--bundles', 'nsis', '--ci')
if ($sign) {
    # A file, not inline JSON: PowerShell 5 and 7 quote JSON arguments to native commands differently.
    $updaterConfig = Join-Path ([IO.Path]::GetTempPath()) 'remotedesktop-updater.json'
    '{"bundle":{"createUpdaterArtifacts":true}}' | Set-Content -Encoding ascii $updaterConfig
    $tauriArgs += @('--config', $updaterConfig)
} else {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Force $out | Out-Null
Write-Host "RemoteDesktop $version from $($commit.Substring(0, 12)) -> $out (updater signatures: $sign)"

Step 'UI dependencies'
Run pnpm -C app/ui install --frozen-lockfile

Step "Build $target"
Run rustup target add $target
Run pnpm -C app/ui tauri build @tauriArgs

# --- collect (the names of scripts/collect-bundles.sh) ---
Step "Collect $target"
$installers = @(Get-ChildItem "target\$target\release\bundle\nsis\*-setup.exe")
if ($installers.Count -ne 1) { Die "expected one target\$target\release\bundle\nsis\*-setup.exe, found $($installers.Count)" }
$dest = Join-Path $out "RemoteDesktop_${version}_windows_x64-setup.exe"
Copy-Item $installers[0].FullName $dest -Force
Write-Host $dest
if ($sign) {
    $sig = "$($installers[0].FullName).sig"
    if (-not (Test-Path $sig)) { Die "$($installers[0].Name) has no updater signature (.sig)" }
    Copy-Item $sig "$dest.sig" -Force
    Write-Host "$dest.sig"
}
# The commit the bundles came from; scripts/release-publish.sh checks it.
"$commit`nupdater=$($sign.ToString().ToLower())`n" | Set-Content -NoNewline -Encoding ascii (Join-Path $out ".build-$target")

Step 'Done'
Get-ChildItem "$out\*_windows_*" | Format-Table Name, @{ n = 'MiB'; e = { [math]::Round($_.Length / 1MB, 1) } }
