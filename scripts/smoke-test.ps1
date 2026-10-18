# Launch smoke test (Windows): starts the app, checks that it still runs after N seconds, logs the
# working set of the app and its child processes (WebView2), then stops it.
#
#   powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1 -Exe "<path to .exe>" [-Seconds 15]
#   powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1 -Installer "<setup.exe>"   # installs per user first
param(
    [string]$Exe,
    [string]$Installer,
    [int]$Seconds = 15
)
$ErrorActionPreference = 'Stop'

if ($Installer) {
    Start-Process -FilePath $Installer -ArgumentList '/S' -Wait
    $dir = Join-Path $env:LOCALAPPDATA 'RemoteDesktop'
    $found = Get-ChildItem $dir -Filter *.exe | Where-Object { $_.Name -notmatch 'uninstall' } | Select-Object -First 1
    if (-not $found) { Write-Output "::error::nothing installed in $dir"; Get-ChildItem $env:LOCALAPPDATA; exit 1 }
    $Exe = $found.FullName
}
if (-not $Exe) { Write-Output 'usage: smoke-test.ps1 -Exe <path> | -Installer <setup.exe>'; exit 2 }

$process = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds $Seconds

if ($process.HasExited) {
    Write-Output "::error::$(Split-Path $Exe -Leaf) exited within ${Seconds}s (exit code $($process.ExitCode))"
    exit 1
}

# The app plus every descendant (msedgewebview2.exe runs under it).
$all = Get-CimInstance Win32_Process
$ids = [System.Collections.Generic.HashSet[int]]::new()
[void]$ids.Add($process.Id)
do {
    $added = $false
    foreach ($p in $all) {
        if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
    }
} while ($added)
$bytes = ($all | Where-Object { $ids.Contains([int]$_.ProcessId) } | Measure-Object -Property WorkingSetSize -Sum).Sum
$mib = [math]::Round($bytes / 1MB, 1)
$message = "$(Split-Path $Exe -Leaf) is running after ${Seconds}s; working set of its $($ids.Count) processes: $mib MiB"
Write-Output $message
if ($env:GITHUB_STEP_SUMMARY) {
    Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value "- Windows $($env:SMOKE_LABEL): $message"
}

foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
exit 0
