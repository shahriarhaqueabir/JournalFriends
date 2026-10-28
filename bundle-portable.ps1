$ErrorActionPreference = "Stop"

$repoRoot = $PSScriptRoot
$configPath = Join-Path $repoRoot "src-tauri\tauri.conf.json"
$releaseDir = Join-Path $repoRoot "src-tauri\target\release"
$bundleRoot = Join-Path $releaseDir "bundle\portable"
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$version = $config.version
$stageDir = Join-Path $bundleRoot "Arnold-$version"
$zipPath = Join-Path $bundleRoot "Arnold-$version-portable.zip"

$requiredFiles = @(
  (Join-Path $releaseDir "arnold.exe"),
  (Join-Path $releaseDir "arnold_lib.dll"),
  (Join-Path $releaseDir "engines\llama-server.exe"),
  (Join-Path $releaseDir "engines\llama-server-impl.dll"),
  (Join-Path $releaseDir "engines\ggml.dll"),
  (Join-Path $releaseDir "engines\llama.dll")
)
$missing = $requiredFiles | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }
if ($missing) {
  throw "Portable bundle prerequisites missing. Run 'cargo tauri build' first; missing: $($missing -join ', ')"
}

if ((Test-Path -LiteralPath $stageDir) -or (Test-Path -LiteralPath $zipPath)) {
  throw "Portable output already exists. Move or rename it before rebuilding: $stageDir"
}

New-Item -ItemType Directory -Path $stageDir -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $releaseDir "arnold.exe") -Destination $stageDir
Copy-Item -LiteralPath (Join-Path $releaseDir "arnold_lib.dll") -Destination $stageDir
Copy-Item -LiteralPath (Join-Path $releaseDir "engines") -Destination $stageDir -Recurse

$readme = @"
Arnold $version - Portable Windows bundle

Extract this entire folder to a writable location outside the user profile and
protected system folders (for example, a writable app folder on another drive).
The app stores its database, logs, models, backups, exports, and attachments in .\data\.
ARNOLD_DATA_DIR can override that location.

The llama.cpp server runtime is included in .\engines\ with its required DLLs.
GGUF chat and embedding models are not included; import them in the app.
WebView2 Runtime is required.

Existing %APPDATA%\Arnold data is not migrated or deleted.
"@
[System.IO.File]::WriteAllText((Join-Path $stageDir "README_PORTABLE.txt"), $readme)

Compress-Archive -Path (Join-Path $stageDir "*") -DestinationPath $zipPath -CompressionLevel NoCompression

Get-Item -LiteralPath $zipPath | Select-Object FullName, Length