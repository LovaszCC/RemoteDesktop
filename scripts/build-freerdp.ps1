# Builds a static FreeRDP (+ OpenSSL) for the embedded RDP engine (crates/rd-freerdp) on Windows; macOS
# and Linux use scripts/build-freerdp.sh, whose pinned versions and checksums this script reads.
# crates/rd-freerdp/build.rs runs it when the build is missing.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-freerdp.ps1 [-Target x86_64-pc-windows-msvc] [-Force]
#
#   -> third_party\freerdp\build\<freerdp version>\<target triple>\{include,lib}
#
# Needs Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC, nmake, and its
# CMake + Ninja component) and Perl for OpenSSL's Configure (Strawberry Perl: `winget install
# StrawberryPerl.StrawberryPerl`). OpenSSL is built without assembly, so NASM isn't needed; jom (parallel
# nmake) is downloaded by the script.
param(
    [string]$Target = 'x86_64-pc-windows-msvc',
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
# Windows PowerShell's download progress bar slows Invoke-WebRequest down many times over.
$ProgressPreference = 'SilentlyContinue'
# Started with a verbatim path (\\?\C:\...), Join-Path finds no drive; use the plain one.
$root = (Split-Path -Parent $PSScriptRoot) -replace '^\\\\\?\\(?!UNC\\)', ''
Set-Location $root

function Die($message) { Write-Host "error: $message" -ForegroundColor Red; exit 1 }
function Run {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE -ne 0) { Die "$($args -join ' ') failed (exit code $LASTEXITCODE)" }
}

# The pins live in build-freerdp.sh.
$pins = @{}
foreach ($line in Get-Content scripts\build-freerdp.sh) {
    if ($line -match '^(FREERDP_VERSION|FREERDP_SHA256|OPENSSL_VERSION|OPENSSL_SHA256)=(.+)$') { $pins[$Matches[1]] = $Matches[2].Trim() }
}
foreach ($key in 'FREERDP_VERSION', 'FREERDP_SHA256', 'OPENSSL_VERSION', 'OPENSSL_SHA256') {
    if (-not $pins[$key]) { Die "$key not found in scripts\build-freerdp.sh" }
}
if ($Target -ne 'x86_64-pc-windows-msvc') { Die "unsupported target $Target" }

$out = Join-Path $root "third_party\freerdp\build\$($pins.FREERDP_VERSION)\$Target"
$work = Join-Path $root "third_party\freerdp\build\work-$Target"
if ((Test-Path (Join-Path $out '.complete')) -and -not $Force) {
    Write-Host "$out already built (-Force rebuilds)"
    exit 0
}

# The MSVC environment (cl, nmake, and the CMake/Ninja that come with Visual Studio).
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { Die 'Visual Studio Build Tools not found (vswhere.exe missing)' }
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { Die 'no Visual Studio with the C++ tools (Desktop development with C++)' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
}
foreach ($tool in 'cl', 'nmake', 'cmake') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { Die "$tool not found (see the header of this script)" }
}
# Run from Git Bash, PATH puts Git's MSYS tar and perl first: the tar can't read C:\ paths, and OpenSSL's
# Configure needs a native perl. Windows' own tar and Strawberry Perl are taken instead.
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path $tar)) { Die "$tar not found (Windows 10 1803 or newer has it)" }
$perl = @('C:\Strawberry\perl\bin\perl.exe') + @(Get-Command perl -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source }) |
    Where-Object { $_ -and (Test-Path $_) -and $_ -notmatch '\\(usr|mingw64)\\bin\\' } | Select-Object -First 1
if (-not $perl) { Die 'native Perl not found: winget install StrawberryPerl.StrawberryPerl' }
$generator = if (Get-Command ninja -ErrorAction SilentlyContinue) { 'Ninja' } else { 'NMake Makefiles' }

# jom: nmake on every core, for OpenSSL (nmake alone takes 10-20 minutes). Pinned; downloaded once next to
# the FreeRDP builds.
$jomVersion = '1.1.7'
$jomSha256 = '4c8af345586a9a08fbfd2f613fcac748226d91a75627aa3581b297dd513046fe'
$jomDir = Join-Path $root "third_party\freerdp\build\tools\jom-$jomVersion"
$jom = Join-Path $jomDir 'jom.exe'
if (-not (Test-Path $jom)) {
    New-Item -ItemType Directory -Force $jomDir | Out-Null
    $zip = Join-Path $jomDir 'jom.zip'
    Invoke-WebRequest -Uri "https://download.qt.io/official_releases/jom/jom_$($jomVersion -replace '\.', '_').zip" -OutFile $zip -UseBasicParsing
    if ((Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower() -ne $jomSha256) { Remove-Item -Recurse -Force $jomDir; Die 'checksum mismatch for jom' }
    Expand-Archive -Path $zip -DestinationPath $jomDir -Force
    Remove-Item $zip
}

Remove-Item -Recurse -Force $work, $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $work 'src'), $out | Out-Null
$prefix = Join-Path $work 'prefix'

function Fetch($url, $sha256, $file) {
    $path = Join-Path $work "src\$file"
    Invoke-WebRequest -Uri $url -OutFile $path -UseBasicParsing
    $actual = (Get-FileHash -Algorithm SHA256 $path).Hash.ToLower()
    if ($actual -ne $sha256) { Die "checksum mismatch for $file" }
    Run $tar -xzf $path -C (Join-Path $work 'src')
}

Write-Host "==> OpenSSL $($pins.OPENSSL_VERSION) ($Target)"
Fetch "https://github.com/openssl/openssl/releases/download/openssl-$($pins.OPENSSL_VERSION)/openssl-$($pins.OPENSSL_VERSION).tar.gz" $pins.OPENSSL_SHA256 'openssl.tar.gz'
Push-Location (Join-Path $work "src\openssl-$($pins.OPENSSL_VERSION)")
# no-module: providers are built into libcrypto; no-asm: no NASM needed.
Run $perl Configure VC-WIN64A no-shared no-tests no-docs no-apps no-module no-asm "--prefix=$prefix" "--openssldir=$prefix\ssl"
# /FS: the parallel cl processes share one .pdb.
$env:CL = '/FS'
Run $jom /J $env:NUMBER_OF_PROCESSORS build_libs
Remove-Item Env:CL
Run nmake install_dev
Pop-Location

Write-Host "==> FreeRDP $($pins.FREERDP_VERSION) ($Target)"
Fetch "https://github.com/FreeRDP/FreeRDP/releases/download/$($pins.FREERDP_VERSION)/freerdp-$($pins.FREERDP_VERSION).tar.gz" $pins.FREERDP_SHA256 'freerdp.tar.gz'
$src = Join-Path $work "src\freerdp-$($pins.FREERDP_VERSION)"
$options = Get-Content scripts\freerdp-cmake-options.txt | Where-Object { $_ -like '-D*' }
# USE_VERSION_FROM_GIT_TAG=OFF: the sources sit inside this checkout; `git describe` would take our tag.
Run cmake -S $src -B (Join-Path $work 'build') -G $generator `
    -DCMAKE_BUILD_TYPE=Release `
    -DUSE_VERSION_FROM_GIT_TAG=OFF `
    -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL `
    "-DCMAKE_INSTALL_PREFIX=$prefix" `
    "-DCMAKE_PREFIX_PATH=$prefix" `
    "-DOPENSSL_ROOT_DIR=$prefix" `
    -DOPENSSL_USE_STATIC_LIBS=ON `
    -DOPENSSL_MSVC_STATIC_RT=OFF `
    @options
Run cmake --build (Join-Path $work 'build') --config Release
Run cmake --install (Join-Path $work 'build') --config Release

New-Item -ItemType Directory -Force (Join-Path $out 'lib'), (Join-Path $out 'include') | Out-Null
Copy-Item (Join-Path $prefix 'lib\*.lib') (Join-Path $out 'lib')
# Build-tree-only static libraries (client channel helpers), each once.
Get-ChildItem (Join-Path $work 'build') -Recurse -Filter *.lib | Where-Object { $_.FullName -notmatch 'CMakeFiles' } | ForEach-Object {
    $dest = Join-Path $out "lib\$($_.Name)"
    if (-not (Test-Path $dest)) { Copy-Item $_.FullName $dest }
}
foreach ($dir in 'freerdp3', 'winpr3', 'openssl') {
    Copy-Item -Recurse (Join-Path $prefix "include\$dir") (Join-Path $out 'include')
}
Set-Content -NoNewline -Encoding ascii (Join-Path $out 'VERSION') $pins.FREERDP_VERSION
Copy-Item scripts\freerdp-cmake-options.txt (Join-Path $out 'OPTIONS')
New-Item -ItemType File (Join-Path $out '.complete') | Out-Null
Remove-Item -Recurse -Force $work
Write-Host "Built $out"
Get-ChildItem (Join-Path $out 'lib') | ForEach-Object { $_.Name }
