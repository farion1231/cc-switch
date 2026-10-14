param(
    [ValidateSet('Plan', 'Restart')][string]$Mode = 'Restart',
    [ValidateRange(0, 20)][int]$ExitTimeoutSeconds = 20,
    [ValidateRange(0, 25)][int]$StartTimeoutSeconds = 25
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

# Resolve the installed package rather than matching a process name. Newer Codex
# releases use ChatGPT.exe; ordinary Codex CLI processes must remain untouched.
$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if (-not $package) { throw 'Codex desktop (OpenAI.Codex) is not installed for this Windows user.' }
$manifest = Get-AppxPackageManifest -Package $package.PackageFullName
$application = @($manifest.Package.Applications.Application) | Where-Object {
    [IO.Path]::GetFileName([string]$_.Executable) -in @('Codex.exe', 'ChatGPT.exe')
} | Select-Object -First 1
if (-not $application) { throw 'Cannot identify the Codex desktop executable in the installed package.' }
$executable = [IO.Path]::GetFullPath((Join-Path $package.InstallLocation ([string]$application.Executable)))
$packageRoot = [IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\') + '\'
if (-not $executable.StartsWith($packageRoot, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw 'The Codex desktop executable is outside its installed package or unavailable.'
}
$appId = $package.PackageFamilyName + '!' + [string]$application.Id
function Get-DesktopProcesses {
    @(Get-Process -Name Codex, ChatGPT -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.Equals($executable, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
    })
}
$before = @(Get-DesktopProcesses)
if ($Mode -eq 'Plan') {
    @{ app_id = $appId; executable = $executable; pids = @($before | ForEach-Object Id); windows = @($before | Where-Object MainWindowHandle -NE 0 | ForEach-Object Id) } | ConvertTo-Json -Compress
    return
}
if ($before.Count -gt 0) {
    $windows = @($before | Where-Object MainWindowHandle -NE 0)
    if ($windows.Count -eq 0) { throw 'Codex desktop is running without an accessible window. Quit it from its menu or tray, then retry.' }
    foreach ($process in $windows) {
        if (-not $process.CloseMainWindow()) { throw 'Cannot request Codex desktop to close. Quit it from its menu and retry.' }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds($ExitTimeoutSeconds)
    while (@(Get-DesktopProcesses).Count -gt 0) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Codex desktop did not exit within 20 seconds. Finish or cancel its running work, quit it from its menu, then retry. It was not forcibly terminated.' }
        Start-Sleep -Milliseconds 200
    }
}
# Activate the MSIX application through Windows, preserving its package identity.
Start-Process -FilePath explorer.exe -ArgumentList ('shell:AppsFolder\' + $appId) -WindowStyle Hidden
$deadline = [DateTime]::UtcNow.AddSeconds($StartTimeoutSeconds)
do {
    $after = @(Get-DesktopProcesses | Where-Object MainWindowHandle -NE 0)
    if ($after.Count -gt 0) {
        @{ action = $(if ($before.Count -gt 0) { 'restarted' } else { 'started' }); desktop_pid = $after[0].Id } | ConvertTo-Json -Compress
        return
    }
    if ([DateTime]::UtcNow -ge $deadline) { throw 'Codex desktop did not open a new window within 25 seconds. Open it from the Start menu and retry.' }
    Start-Sleep -Milliseconds 200
} while ($true)
