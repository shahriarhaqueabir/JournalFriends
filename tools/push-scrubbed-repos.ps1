# =============================================================================
# push-scrubbed-repos.ps1
# -----------------------------------------------------------------------------
# Force-pushes the ALREADY-REWRITTEN mirrors produced by scrub-public-repo-email.ps1.
# git-filter-repo removes the 'origin' remote after rewriting, so this script:
#   1. re-adds origin pointing at the public repo
#   2. re-verifies NO old email remains in history
#   3. git push --force --mirror origin  (with real exit-code checking)
#
# USAGE: pwsh -File tools/push-scrubbed-repos.ps1 [-Repo <name>]
# =============================================================================

param([string]$Repo = "")

$ErrorActionPreference = "Stop"

$OLD_EMAILS = @("shahriar_abby@hotmail.com", "shahr@example.com")
$REPOS = @(
  "UniversalOps",
  "PowershellOps",
  "AI-Assisted-German-Law",
  "LovelyGarden",
  "ShahriarHaqueAbirPortfolio",
  "AllOpsFull",
  "de_laws_to_json"
)
$TMP = Join-Path $env:TEMP "email-scrub"

function Write-Step([string]$msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

$targets = if ($Repo) { @($Repo) } else { $REPOS }

foreach ($name in $targets) {
  if ($name -notin $REPOS) { Write-Warning "Skipping unknown repo: $name"; continue }
  Write-Step "=== $name ==="

  $work = Join-Path $TMP "$name.git"
  if (-not (Test-Path $work)) {
    Write-Warning "Rewritten mirror missing: $work  (run scrub-public-repo-email.ps1 first)"; continue
  }

  Push-Location $work
  try {
    # Safety: never push unless the rewrite actually removed the old emails
    $emails = git log --all --format="%ae" | Sort-Object -Unique
    if (($emails -join ",") -match "hotmail|example\.com") {
      Write-Warning "OLD EMAIL STILL PRESENT in $name - refusing to push"; continue
    }
    Write-Step ("Verified clean. Emails: " + ($emails -join ", "))

    # Re-add the remote that filter-repo stripped
    git remote remove origin 2>$null
    git remote add origin "https://github.com/shahriarhaqueabir/$name.git"

    Write-Step "Force-pushing rewritten history..."
    $pushOut = git push --force --mirror origin 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0) {
      # Protected refs (main/tags) will be rejected by repo rulesets - capture and report
      $rejected = ($pushOut | Select-String -Pattern "rejected").Line
      Write-Warning "PUSH PARTIAL/FAILED for $name (exit $code)"
      $rejected | ForEach-Object { Write-Host "  $_" -ForegroundColor Yellow }
    } else {
      Write-Step "Pushed $name OK"
    }
  } finally {
    Pop-Location
  }
}

Write-Host ""
Write-Step "DONE."
