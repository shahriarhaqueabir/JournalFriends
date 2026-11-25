param(
  [ValidateSet('fresh', 'upgrade')]
  [string]$Profile = 'fresh',
  [string]$Exe,
  [switch]$NoLaunch
)

# Launches the release build against an isolated data directory so manual E2E
# runs never touch real user data.
#
#   .\tools\test-run.ps1 -Profile fresh      # clean first-run (onboarding + vault create)
#   .\tools\test-run.ps1 -Profile upgrade    # copy of the legacy vault DB (migrations 015/016/017)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'src-tauri\target\release\arnold.exe' }
if (-not (Test-Path $Exe)) { throw "Release build not found: $Exe  (run: cargo tauri build --bundles msi nsis)" }

$dataDir = Join-Path $repo "test-data\$Profile"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

if ($Profile -eq 'upgrade') {
  $legacyAppName = if ($env:JOURNALFRIENDS_LEGACY_APPDATA) { $env:JOURNALFRIENDS_LEGACY_APPDATA } else { 'JournalFriendsLegacy' }
  $src = Join-Path $env:APPDATA $legacyAppName
  $legacyPrefix = if ($env:JOURNALFRIENDS_LEGACY_PREFIX) { $env:JOURNALFRIENDS_LEGACY_PREFIX } else { 'journalfriends' }
  $map = @{
    "$legacyPrefix.db"      = 'arnold.db'
    "$legacyPrefix.db-wal"  = 'arnold.db-wal'
    "$legacyPrefix.db-shm"  = 'arnold.db-shm'
    "$legacyPrefix.db.salt" = 'arnold.db.salt'
  }
  foreach ($from in $map.Keys) {
    $f = Join-Path $src $from
    if (Test-Path $f) { Copy-Item $f (Join-Path $dataDir $map[$from]) -Force }
  }
  if (-not (Test-Path (Join-Path $dataDir 'arnold.db'))) {
    throw "No legacy DB found under $src - nothing to upgrade-test. Set JOURNALFRIENDS_LEGACY_APPDATA/JOURNALFRIENDS_LEGACY_PREFIX to point at legacy data."
  }
  Write-Host "Upgrade profile seeded from $src (legacy password required on unlock)."
}

# Reap orphaned engines left by a force-killed previous run: they keep
# :8080/:8081 bound, so the next launch fails provisioning with
# "port is already in use by another process" even though no app is running.
$orphans = @(
  Get-CimInstance Win32_Process -Filter "Name='llama-server.exe'" -ErrorAction SilentlyContinue |
    Where-Object { -not (Get-Process -Id $_.ParentProcessId -ErrorAction SilentlyContinue) }
)
if ($orphans.Count -gt 0) {
  $orphans | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Write-Host "Reaped $($orphans.Count) orphaned llama-server process(es) from a previous run."
}

Write-Host "Profile : $Profile"
Write-Host "Exe     : $Exe"
Write-Host "Data    : $dataDir"
Write-Host "Log     : $(Join-Path $dataDir 'arnold-debug.log')"
Write-Host "Models  : $repo\llama-b10952-bin-win-cuda-x64\models"
Write-Host "Engine  : $repo\llama-b10952-bin-win-cuda-x64\llama-server.exe"

if ($NoLaunch) { return }

$env:ARNOLD_DATA_DIR = $dataDir
Start-Process -FilePath $Exe -WorkingDirectory (Split-Path -Parent $Exe) | Out-Null
Write-Host 'Launched.'
