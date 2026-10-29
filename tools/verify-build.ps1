param(
  [string]$Repo = (Split-Path -Parent $PSScriptRoot)
)

# Verifies a release build: installer artifacts, embedded offline WebView2 setup,
# and the packaged llama.cpp runtime. Prints PASS/FAIL per check.

$ErrorActionPreference = 'Continue'
$rel = Join-Path $Repo 'src-tauri\target\release'
$fail = 0

function Check([string]$name, [bool]$ok, [string]$detail) {
  if ($ok) { Write-Host ("PASS  {0,-46} {1}" -f $name, $detail) -ForegroundColor Green }
  else { Write-Host ("FAIL  {0,-46} {1}" -f $name, $detail) -ForegroundColor Red; $script:fail++ }
}

function Size-MiB($bytes) { '{0:N1} MiB' -f ($bytes / 1MB) }

# --- installers ---
$msi = Get-ChildItem (Join-Path $rel 'bundle\msi') -Filter *.msi -ErrorAction SilentlyContinue | Select-Object -First 1
$exe = Get-ChildItem (Join-Path $rel 'bundle\nsis') -Filter *.exe -ErrorAction SilentlyContinue | Select-Object -First 1
Check 'MSI installer exists' ($null -ne $msi) $(if ($msi) { "$($msi.Name) $(Size-MiB $msi.Length)" } else { 'missing' })
Check 'NSIS installer exists' ($null -ne $exe) $(if ($exe) { "$($exe.Name) $(Size-MiB $exe.Length)" } else { 'missing' })
Check 'MSI embeds offline WebView2 (>600 MiB)' ($msi -and $msi.Length -gt 600MB) $(if ($msi) { Size-MiB $msi.Length } else { 'n/a' })
Check 'NSIS embeds offline WebView2 (>600 MiB)' ($exe -and $exe.Length -gt 600MB) $(if ($exe) { Size-MiB $exe.Length } else { 'n/a' })

# --- wix / nsis config ---
$nsi = Join-Path $rel 'nsis\x64\installer.nsi'
if (Test-Path $nsi) {
  $nsiText = Get-Content $nsi -Raw
  Check 'installer.nsi uses offlineInstaller' ($nsiText -match 'INSTALLWEBVIEW2MODE\s+"offlineInstaller"') 'target/release/nsis/x64/installer.nsi'
} else { Check 'installer.nsi present' $false 'missing' }

$wxs = Join-Path $rel 'wix\x64\main.wxs'
if (Test-Path $wxs) {
  $wxsText = Get-Content $wxs -Raw
  $hasBinary = $wxsText -match 'MicrosoftEdgeWebView2RuntimeInstaller'
  $hasAction = $wxsText -match 'InvokeStandalone'
  Check 'main.wxs embeds WebView2 runtime' ($hasBinary -and $hasAction) "binary=$hasBinary action=$hasAction"
} else { Check 'main.wxs present' $false 'missing' }

# --- app + packaged runtime ---
$appExe = Join-Path $rel 'arnold.exe'
Check 'Release arnold.exe exists' (Test-Path $appExe) $(if (Test-Path $appExe) { Size-MiB (Get-Item $appExe).Length } else { 'missing' })

$engines = Join-Path $rel 'engines'
$server = Join-Path $engines 'llama-server.exe'
Check 'Packaged engines\llama-server.exe' (Test-Path $server) $server
if (Test-Path $engines) {
  $dlls = @(Get-ChildItem $engines -Filter *.dll)
  $implDll = Test-Path (Join-Path $engines 'llama-server-impl.dll')
  $license = Test-Path (Join-Path $engines 'LICENSE-LLVM-OpenMP')
  Check 'Packaged runtime DLL set' ($dlls.Count -ge 20) "$($dlls.Count) DLLs"
  Check 'llama-server-impl.dll packaged' $implDll 'required by the 9 KB launcher exe'
  Check 'LLVM/OpenMP license packaged' $license 'LICENSE-LLVM-OpenMP'
}
if (Test-Path $server) {
  $ver = & $server --version 2>&1 | Out-String
  $line = ($ver -split "`r?`n" | Where-Object { $_ -match 'build \d+' } | Select-Object -First 1)
  Check 'Packaged runtime reports build 10952' ($ver -match 'build 10952') $line.Trim()
}

# --- hashes ---
if ($msi) { Write-Host ("MSI  SHA256 {0}" -f (Get-FileHash $msi.FullName -Algorithm SHA256).Hash) }
if ($exe) { Write-Host ("NSIS SHA256 {0}" -f (Get-FileHash $exe.FullName -Algorithm SHA256).Hash) }

Write-Host ''
if ($fail -eq 0) { Write-Host 'ALL BUILD CHECKS PASSED' -ForegroundColor Green } else { Write-Host "$fail CHECK(S) FAILED" -ForegroundColor Red }
exit $fail
