<#
.SYNOPSIS
  Merge official upstream (origin/main) into the current feature branch without
  pushing your custom work to the official repo.

.DESCRIPTION
  Remotes expected:
    origin -> https://github.com/farion1231/cc-switch.git   (official, fetch only)
    fork   -> https://github.com/xjwm5685-ui/cc-switch-pro.git (your product line)

  Default flow:
    1) refuse dirty worktree unless -Stash
    2) fetch origin
    3) merge origin/main into HEAD
    4) optionally push to fork (-Push)

.EXAMPLE
  pnpm sync:upstream
  pwsh -File scripts/sync-upstream.ps1 -Stash -Push
#>
[CmdletBinding()]
param(
  [string]$UpstreamRemote = "origin",
  [string]$UpstreamBranch = "main",
  [string]$ForkRemote = "fork",
  [switch]$Stash,
  [switch]$Push,
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Assert-Command([string]$Name) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "Required command not found: $Name"
  }
}

Assert-Command git

$repoRoot = (git rev-parse --show-toplevel 2>$null)
if (-not $repoRoot) {
  throw "Not inside a git repository."
}
Set-Location $repoRoot

$currentBranch = (git branch --show-current).Trim()
if (-not $currentBranch) {
  throw "Detached HEAD is not supported. Checkout a branch first (e.g. feat/pi-support)."
}

Write-Step "Current branch: $currentBranch"

$remotes = git remote
if ($remotes -notcontains $UpstreamRemote) {
  throw "Missing remote '$UpstreamRemote'. Expected official farion1231/cc-switch."
}
if ($Push -and ($remotes -notcontains $ForkRemote)) {
  throw "Missing remote '$ForkRemote'. Add it before -Push."
}

# Never push custom work to the official remote by accident.
$upstreamUrl = (git remote get-url $UpstreamRemote)
if ($Push -and $upstreamUrl -match "farion1231/cc-switch") {
  Write-Host "Safety: -Push will target '$ForkRemote', never '$UpstreamRemote' ($upstreamUrl)" -ForegroundColor DarkYellow
}

$status = git status --porcelain
$didStash = $false
if ($status) {
  if (-not $Stash) {
    Write-Host "Working tree is dirty. Commit/stash first, or re-run with -Stash." -ForegroundColor Red
    git status -sb
    exit 1
  }
  Write-Step "Stashing local changes"
  if (-not $DryRun) {
    git stash push -u -m "sync-upstream auto-stash $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    $didStash = $true
  }
}

Write-Step "Fetching $UpstreamRemote"
if (-not $DryRun) {
  git fetch $UpstreamRemote --prune
}

$upstreamRef = "$UpstreamRemote/$UpstreamBranch"
$upstreamExists = git rev-parse --verify --quiet $upstreamRef
if (-not $upstreamExists) {
  throw "Upstream ref not found: $upstreamRef"
}

$behind = (git rev-list --count "HEAD..$upstreamRef").Trim()
$ahead = (git rev-list --count "$upstreamRef..HEAD").Trim()
Write-Host "Relative to ${upstreamRef}: behind=$behind, ahead=$ahead"

if ($behind -eq "0") {
  Write-Host "Already up to date with $upstreamRef." -ForegroundColor Green
} else {
  Write-Step "Merging $upstreamRef into $currentBranch"
  if ($DryRun) {
    Write-Host "[dry-run] git merge --no-edit $upstreamRef"
  } else {
    git merge --no-edit $upstreamRef
    if ($LASTEXITCODE -ne 0) {
      Write-Host ""
      Write-Host "Merge conflicts detected. Resolve them, then:" -ForegroundColor Yellow
      Write-Host "  git add -A"
      Write-Host "  git commit"
      if ($didStash) {
        Write-Host "  git stash pop"
      }
      if ($Push) {
        Write-Host "  git push $ForkRemote HEAD"
      }
      exit 1
    }
  }
}

if ($Push) {
  Write-Step "Pushing $currentBranch to $ForkRemote (not $UpstreamRemote)"
  if ($DryRun) {
    Write-Host "[dry-run] git push $ForkRemote HEAD"
  } else {
    git push $ForkRemote "HEAD:refs/heads/$currentBranch"
  }
}

if ($didStash) {
  Write-Step "Restoring stashed changes"
  git stash pop
  if ($LASTEXITCODE -ne 0) {
    Write-Host "stash pop had conflicts. Run: git status" -ForegroundColor Yellow
    exit 1
  }
}

Write-Host ""
Write-Host "Done. Official updates are on '$currentBranch'; your custom commits stay on this branch." -ForegroundColor Green
Write-Host "Reminder: app updater should point at your fork releases, not farion1231." -ForegroundColor DarkYellow
