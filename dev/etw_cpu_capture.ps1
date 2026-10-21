#requires -RunAsAdministrator
# Read-only ETW CPU-sampling capture for the page-decrypt applier.
# Does not open, write to, or inject into the game process.
#
# Usage (elevated PowerShell):
#   powershell -ExecutionPolicy Bypass -File dev\etw_cpu_capture.ps1
#   powershell -ExecutionPolicy Bypass -File dev\etw_cpu_capture.ps1 -Seconds 90
#
# Start this BEFORE launching Overwatch. The first-minute decrypt burst is the
# target; a sample whose leaf PC lands in the loader's pad family names the
# applier even when the stack is broken.

param(
    [string]$Out = "D:\ow-dumps\etw",
    [int]$Seconds = 0
)

$ErrorActionPreference = "Stop"
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($id)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Not elevated. Re-run from an Administrator PowerShell."
    exit 1
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$etl = Join-Path $Out ("cpu_" + $stamp + ".etl")

Write-Host "Starting WPR CPU sampling (sampled profile + stacks)."
Write-Host "Output: $etl"
& wpr -start CPU -filemode
if ($LASTEXITCODE -ne 0) {
    Write-Error "wpr -start failed with $LASTEXITCODE"
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "Trace is running. Launch Overwatch now and enter the practice range."
Write-Host "Stay in the range for at least 30 s so the loader's on-demand"
Write-Host "decrypts are sampled. Then come back to this window."
Write-Host ""
if ($Seconds -gt 0) {
    Write-Host "Stopping automatically in $Seconds seconds..."
    Start-Sleep -Seconds $Seconds
} else {
    Read-Host "Press Enter to stop the trace"
}

& wpr -stop $etl
if ($LASTEXITCODE -ne 0) {
    Write-Error "wpr -stop failed with $LASTEXITCODE"
    exit $LASTEXITCODE
}

$size = (Get-Item -LiteralPath $etl).Length
Write-Host ""
Write-Host "ETL written: $etl ($size bytes)"
Write-Host "Tell the analysis side this path. Do not delete it until the scan is done."
