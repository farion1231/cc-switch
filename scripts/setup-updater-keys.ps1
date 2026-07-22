<#
.SYNOPSIS
  为本 fork 生成 Tauri updater 签名密钥，并打印后续步骤。

.EXAMPLE
  pnpm tauri:signer:generate
  pwsh -File scripts/setup-updater-keys.ps1
  pwsh -File scripts/setup-updater-keys.ps1 -Force
#>
[CmdletBinding()]
param(
  [string]$KeyPath = $(Join-Path $env:USERPROFILE ".tauri\cc-switch-pro.key"),
  [switch]$Force
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
  throw "pnpm not found"
}

$keyDir = Split-Path -Parent $KeyPath
if (-not (Test-Path $keyDir)) {
  New-Item -ItemType Directory -Path $keyDir | Out-Null
}

if ((Test-Path $KeyPath) -and -not $Force) {
  Write-Host "Key already exists: $KeyPath" -ForegroundColor Yellow
  Write-Host "Re-run with -Force to overwrite, or delete the file first."
  if (Test-Path "$KeyPath.pub") {
    Write-Host ""
    Write-Host "Current PUBLIC key (paste into tauri.conf.json plugins.updater.pubkey):" -ForegroundColor Cyan
    Get-Content "$KeyPath.pub" -Raw
  }
  exit 1
}

$forceArgs = @()
if ($Force) { $forceArgs = @("-f") }

Write-Host "Generating updater keypair at $KeyPath ..." -ForegroundColor Cyan
# --ci 非交互；空密码便于本机/CI（生产可自行加密码）
& pnpm tauri signer generate -w $KeyPath --ci -p "" @forceArgs
if ($LASTEXITCODE -ne 0) {
  throw "tauri signer generate failed"
}

$pub = Get-Content "$KeyPath.pub" -Raw
Write-Host ""
Write-Host "PUBLIC KEY (already written to $KeyPath.pub):" -ForegroundColor Green
Write-Host $pub.Trim()
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Green
Write-Host "1) Put the PUBLIC key into src-tauri/tauri.conf.json -> plugins.updater.pubkey"
Write-Host "2) On GitHub repo xjwm5685-ui/cc-switch-pro → Settings → Secrets → Actions, add:"
Write-Host "     TAURI_SIGNING_PRIVATE_KEY          = full contents of $KeyPath"
Write-Host "     TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (leave empty if you used empty -p)"
Write-Host "3) endpoints already point at this fork releases/latest.json"
Write-Host "4) NEVER commit the .key private file"
Write-Host ""
Write-Host "See docs/fork-sync.md"
