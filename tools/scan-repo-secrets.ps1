# Secret scan across all mirror clones (git history + all refs)
# Usage: pwsh tools/scan-repo-secrets.ps1
$ErrorActionPreference = "Continue"
$base = "$env:TEMP\email-scrub"
$repos = @("UniversalOps","PowershellOps","AI-Assisted-German-Law","LovelyGarden","ShahriarHaqueAbirPortfolio")

# High-signal secret patterns (real keys, not generic words)
$secretPatterns = @(
    'ghp_[0-9A-Za-z]{36}',            # GitHub PAT
    'github_pat_[0-9A-Za-z_]{22,}',
    'AKIA[0-9A-Z]{16}',               # AWS access key
    'sk-[A-Za-z0-9]{20,}',            # OpenAI/Anthropic style
    'sk_live_[0-9A-Za-z]{16,}',       # Stripe
    'xox[baprs]-[0-9A-Za-z-]{10,}',   # Slack tokens
    'AIza[0-9A-Za-z_-]{35}',          # Google API key
    '-----BEGIN (RSA |OPENSSH |EC |DSA )?PRIVATE KEY',
    'client_secret\s*[:=]\s*["'']?[0-9A-Za-z_-]{16,}',
    'api[_-]?key\s*[:=]\s*["'']?[0-9A-Za-z_-]{20,}',
    'password\s*[:=]\s*["'']?[^"'']{6,}["'']',
    'secret\s*[:=]\s*["'']?[0-9A-Za-z_-]{16,}["'']'
)

# Sensitive filenames
$sensitiveFiles = '\.env(\.|$)|\.pem$|\.key$|id_rsa|credential|\.pfx$|\.p12$|secrets?\b|token'

foreach ($repo in $repos) {
    $g = Join-Path $base "$repo.git"
    if (-not (Test-Path $g)) { Write-Host "SKIP $repo (no mirror)"; continue }
    Write-Host "`n================ $repo ================"

    # 1. Sensitive filenames in history
    Write-Host "`n--- Sensitive filenames ever in history:"
    $files = git --git-dir=$g log --all --name-only --pretty=format: 2>$null | Where-Object { $_ -and $_ -notmatch '^src-tauri/target|^node_modules|^\.git' }
    $files | Sort-Object -Unique | Select-String -Pattern $sensitiveFiles | Select-Object -ExpandProperty Line -Unique

    # 2. Secret patterns in full history diff output
    foreach ($pat in $secretPatterns) {
        $hits = git --git-dir=$g log --all -p --no-color 2>$null | Select-String -Pattern $pat -AllMatches
        if ($hits) {
            $sample = ($hits | Select-Object -First 5 | ForEach-Object { $_.Line.Trim().Substring(0, [Math]::Min(120, $_.Line.Trim().Length)) })
            Write-Host "PATTERN [$pat]: $($hits.Count) lines"
            $sample | ForEach-Object { Write-Host "    $_" }
        }
    }

    # 3. Personal email addresses in history (non-noreply)
    Write-Host "`n--- Personal emails in commit authors (all refs):"
    git --git-dir=$g log --all --format='%ae|%an' 2>$null | Sort-Object -Unique | Where-Object { $_ -match '@' -and $_ -notmatch 'noreply|dependabot|replit|vercel|pm\.me|users\.noreply' }

    # 4. Config files with potential secrets in current tree of default branch + tags
    Write-Host "`n--- .env / config files at HEAD:"
    git --git-dir=$g ls-tree -r --name-only HEAD 2>$null | Select-String -Pattern '\.env(\.|$)|config\.(json|yml|yaml|toml|ini|js)$|settings\.json$' | Select-Object -First 20
}
Write-Host "`nDONE."
