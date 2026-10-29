param(
  [ValidateSet('fresh', 'upgrade')]
  [string]$Profile = 'fresh'
)

# Prints one-screen evidence for a running test session: process, ports, log health,
# and the on-disk data root. Safe to run any time (read-only).

$ErrorActionPreference = 'Continue'
$repo = Split-Path -Parent $PSScriptRoot
$data = Join-Path $repo "test-data\$Profile"
$log = Join-Path $data 'arnold-debug.log'

Write-Host "=== Arnold smoke check ($Profile) ===" -ForegroundColor Cyan
Write-Host "data: $data"

$app = Get-Process arnold -ErrorAction SilentlyContinue
Write-Host ("app process      : " + $(if ($app) { "running (pid $($app.Id -join ', '))" } else { 'not running' }))

foreach ($p in 8080, 8081) {
  $up = $false
  try { $r = Invoke-RestMethod -Uri "http://127.0.0.1:$p/health" -TimeoutSec 2; $up = $true } catch { $up = $false }
  Write-Host ("engine :{0}       : {1}" -f $p, $(if ($up) { "healthy $($r.status)" } else { 'not responding' }))
}

$servers = Get-Process llama-server -ErrorAction SilentlyContinue
Write-Host ("llama-server procs: " + $(if ($servers) { ($servers | ForEach-Object { "$($_.Id)@$($_.StartTime.ToString('HH:mm:ss'))" }) -join ', ' } else { 'none' }))

Write-Host ''
Write-Host '--- data root ---'
if (Test-Path $data) {
  Get-ChildItem $data -Force | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String | Write-Host
} else { Write-Host 'missing' }

Write-Host '--- log health ---'
if (Test-Path $log) {
  $lines = Get-Content $log
  Write-Host ("lines: {0}  last write: {1}" -f $lines.Count, (Get-Item $log).LastWriteTime)
  $patterns = @{
    panic                    = 'panic'
    foreign_key              = 'FOREIGN KEY'
    missing_archived_field   = "missing field .archived."
    priority_check           = 'CHECK constraint failed: priority'
    embeddings_501           = '501 Not Implemented|501 Unimplemented|HTTP 501|status(?::|=)\s*501'
    network_denied           = 'network error|dns error|connection refused'
  }
  foreach ($k in $patterns.Keys) {
    $hits = @($lines | Select-String -Pattern $patterns[$k])
    $color = if ($hits.Count -eq 0) { 'Green' } else { 'Red' }
    Write-Host ("{0,-24}: {1}" -f $k, $hits.Count) -ForegroundColor $color
  }
  Write-Host ''
  Write-Host 'last 8 lines:'
  $lines | Select-Object -Last 8 | ForEach-Object { Write-Host "  $_" }
} else { Write-Host 'no log yet' }
