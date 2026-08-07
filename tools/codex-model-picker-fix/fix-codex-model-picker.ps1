[CmdletBinding()]
param(
    [string]$ModelIds = "deepseek-v4-flash-0731,glm-5.2"
)

$ErrorActionPreference = 'Stop'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent())
    .IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    Write-Host "Requesting administrator privileges to update the hosts file..."
    $argsFlat = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-ModelIds', "`"$ModelIds`"")
    Start-Process -FilePath 'powershell.exe' -ArgumentList $argsFlat -Verb RunAs -Wait
    exit $LASTEXITCODE
}

$codexProcess = Get-Process -Name 'codex' -ErrorAction SilentlyContinue
if ($codexProcess) {
    Write-Host "Codex is still running. Fully quit Codex (including the system tray icon), then run this script again."
    exit 1
}

$pkgRoot = Join-Path $env:LOCALAPPDATA 'Packages'
$pkg = Get-ChildItem -Path $pkgRoot -Directory -Filter 'OpenAI.Codex_*' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $pkg) {
    Write-Host "Codex package folder not found under $pkgRoot"
    exit 1
}

$leveldb = Join-Path $pkg.FullName 'LocalCache\Roaming\Codex\web\Codex\Default\Local Storage\leveldb'
if (-not (Test-Path -LiteralPath $leveldb)) {
    Write-Host "LevelDB folder not found: $leveldb"
    exit 1
}

# Block the remote Statsig endpoint so Codex cannot refresh the allowlist over
# the network and overwrite the patched cache.
$hostsFile = 'C:\Windows\System32\drivers\etc\hosts'
$hostsLine = '127.0.0.1 ab.chatgpt.com'
$hostsContent = Get-Content -LiteralPath $hostsFile -Raw
if ($hostsContent -notmatch 'ab\.chatgpt\.com') {
    Add-Content -LiteralPath $hostsFile -Value "`n$hostsLine" -Encoding Ascii
    Write-Host "Added hosts entry: $hostsLine"
} else {
    Write-Host "Hosts entry already present: $hostsLine"
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$backup = "$leveldb.bak-$stamp"
Copy-Item -LiteralPath $leveldb -Destination $backup -Recurse -Force
Write-Host "Backup created: $backup"

$toolDir = $PSScriptRoot
$nodeModules = Join-Path $toolDir 'node_modules'
if (-not (Test-Path (Join-Path $nodeModules 'classic-level'))) {
    Write-Host "Installing classic-level dependency..."
    Push-Location $toolDir
    try {
        cmd /c "npm install classic-level@1.4.1 --no-audit --no-fund --loglevel=error"
        if ($LASTEXITCODE -ne 0) { throw "npm install failed" }
    } finally {
        Pop-Location
    }
}
$env:NODE_PATH = $nodeModules
$node = (Get-Command node -ErrorAction Stop).Source

& $node (Join-Path $toolDir 'patch-statsig.js') $leveldb $ModelIds
if ($LASTEXITCODE -ne 0) {
    Write-Host "Patching failed. Restore from backup: $backup"
    exit 1
}

Write-Host ""
Write-Host "Patch applied. Relaunch Codex and open the model picker."
Write-Host "The remote Statsig endpoint (ab.chatgpt.com) is blocked, so the allowlist will not be overwritten on restart."
