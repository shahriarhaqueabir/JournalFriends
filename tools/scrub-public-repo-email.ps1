# =============================================================================
# scrub-public-repo-email.ps1
# -----------------------------------------------------------------------------
# Rewrites commit author/committer email out of PUBLIC repo history and
# force-pushes the rewritten history. Run per-repo or for all 7 exposed repos.
#
#   Old:  shahriar_abby@hotmail.com   (and shahr@example.com in PowershellOps)
#   New:  30010179+shahriarhaqueabir@users.noreply.github.com
#
# USAGE
#   pwsh -File tools/scrub-public-repo-email.ps1 -DryRun          # rewrite locally, NO push
#   pwsh -File tools/scrub-public-repo-email.ps1                  # rewrite + force-push ALL 7
#   pwsh -File tools/scrub-public-repo-email.ps1 -Repo UniversalOps   # single repo
#
# REQUIREMENTS
#   - python available (uses .venv in this workspace)
#   - git credential helper set up for github.com (so pushes authenticate)
#
# WARNING
#   - Rewrites commit SHAs. Local clones of these repos will be out of sync
#     afterward; re-clone them (or fetch --force + hard reset).
#   - Existing forks/PRs pointing at old SHAs will be orphaned.
# =============================================================================

param(
  [string]$Repo = "",
  [switch]$DryRun = $false
)

$ErrorActionPreference = "Stop"

$OLD_EMAILS = @("shahriar_abby@hotmail.com", "shahr@example.com")
$NEW_EMAIL  = "30010179+shahriarhaqueabir@users.noreply.github.com"
$NEW_NAME   = "hawkward"

$REPOS = @(
  "UniversalOps",
  "PowershellOps",
  "AI-Assisted-German-Law",
  "LovelyGarden",
  "ShahriarHaqueAbirPortfolio",
  "AllOpsFull",
  "de_laws_to_json",
  "HawkwardJournalsAI"
)

$TMP = Join-Path $env:TEMP "email-scrub"

function Write-Step([string]$msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

# --- 0. Install git-filter-repo into this workspace's venv -------------------
$env:PYTHONPATH = "E:\pip-packages;$env:PYTHONPATH"
python -c "import git_filter_repo" 2>$null
if ($LASTEXITCODE -ne 0) {
  Write-Step "Installing git-filter-repo into .venv..."
  python -m pip install --quiet git-filter-repo
  python -c "import git_filter_repo" 2>$null
  if ($LASTEXITCODE -ne 0) { Write-Error "git-filter-repo install failed"; exit 1 }
}

# Define filter-repo command (handling case where it's a python script)
function git-filter-repo {
  python E:\pip-packages\git_filter_repo.py $args
}

# --- Mailmap: any author carrying the old email -> new name+email -------------
# Email-only form (Proper <new> <old>) matches regardless of author name,
# unlike the name+email form which silently no-ops when the name differs.
$MAILMAP = Join-Path $TMP "mailmap.txt"
New-Item -ItemType Directory -Force -Path $TMP | Out-Null
@"
$NEW_NAME <$NEW_EMAIL> <$($OLD_EMAILS[0])>
$NEW_NAME <$NEW_EMAIL> <$($OLD_EMAILS[1])>
"@ | Set-Content -Path $MAILMAP -Encoding utf8

$targets = if ($Repo) { @($Repo) } else { $REPOS }

foreach ($name in $targets) {
  if ($name -notin $REPOS) { Write-Warning "Skipping unknown repo: $name"; continue }
  Write-Step "=== $name ==="

  $work = Join-Path $TMP "$name.git"
  if (Test-Path $work) { Remove-Item -Recurse -Force $work }

  # Fresh mirror clone (filter-repo refuses non-fresh clones)
  Write-Step "Cloning mirror..."
  git clone --mirror "https://github.com/shahriarhaqueabir/$name.git" $work | Out-Null
  Push-Location $work
  try {
    $before = git log --all --format="%ae" | Sort-Object -Unique
    Write-Step ("Emails present: " + ($before -join ", "))

    Write-Step "Rewriting email -> $NEW_EMAIL"
    git-filter-repo --force --mailmap $MAILMAP | Out-Null

    $after = git log --all --format="%ae" | Sort-Object -Unique
    Write-Step ("After: " + ($after -join ", "))
    if (($after -join ",") -match "hotmail|example\.com") {
      Write-Error "Old email still present after rewrite - aborting push for $name"
      continue
    }
  } finally {
    Pop-Location
  }

  if ($DryRun) {
    Write-Step "DRY RUN: skipping push for $name (rewritten copy at $work)"
  } else {
    Write-Step "Force-pushing rewritten history..."
    Push-Location $work
    try { git push --force --mirror origin } finally { Pop-Location }
    Write-Step "Pushed $name"
  }
}

Write-Host ""
Write-Step "DONE."
if (-not $DryRun) {
  Write-Host "NOTE: Local clones are now out of sync. Re-clone each repo, or run:"
  Write-Host "  git fetch --all --prune; git reset --hard origin/main"
}
