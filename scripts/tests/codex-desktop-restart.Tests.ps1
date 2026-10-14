# Offline lifecycle fixtures. No real package, process, window, or launch is used.
$ErrorActionPreference = 'Stop'
$restartScript = Join-Path $PSScriptRoot '../../src-tauri/src/services/codex_desktop_windows.ps1'
$script:exe = 'C:\Program Files\WindowsApps\OpenAI.Codex_test\app\ChatGPT.exe'
function Get-AppxPackage {
    param($Name)
    if ($script:missing) { return }
    [pscustomobject]@{ Version = [Version]'1.0'; InstallLocation = 'C:\Program Files\WindowsApps\OpenAI.Codex_test'; PackageFullName = 'test'; PackageFamilyName = 'OpenAI.Codex_test' }
}
function Get-AppxPackageManifest {
    param($Package)
    [pscustomobject]@{ Package = [pscustomobject]@{ Applications = [pscustomobject]@{ Application = [pscustomobject]@{ Executable = 'app\ChatGPT.exe'; Id = 'App' } } } }
}
function Test-Path { param($LiteralPath, $PathType) $true }
function Get-Process {
    param($Name, $ErrorAction)
    if ($script:launched) {
        if ($script:launchTimeout) { return }
        return [pscustomobject]@{ Id = 42; Path = $script:exe; MainWindowHandle = 100 }
    }
    if ($script:closed -and -not $script:exitTimeout) { return }
    $desktop = [pscustomobject]@{ Id = 10; Path = $script:exe; MainWindowHandle = $(if ($script:noWindow) { 0 } else { 100 }) }
    $desktop | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value { $script:closed = $true; $true }
    $desktop
    # A CLI with the same process name must never be included in the restart.
    [pscustomobject]@{ Id = 99; Path = 'C:\tools\codex.exe'; MainWindowHandle = 200 }
}
function Start-Process {
    param($FilePath, $ArgumentList, $WindowStyle)
    if ($FilePath -ne 'explorer.exe' -or $ArgumentList -ne 'shell:AppsFolder\OpenAI.Codex_test!App') { throw 'Unexpected desktop activation' }
    $script:launched = $true
}
function Start-Sleep { param($Milliseconds) }
function Reset-Fixture {
    $script:missing = $false; $script:closed = $false; $script:launched = $false
    $script:noWindow = $false; $script:exitTimeout = $false; $script:launchTimeout = $false
}
function Expect-Failure([string]$Expected) {
    try { . $restartScript -ExitTimeoutSeconds 0 -StartTimeoutSeconds 0; throw 'Expected restart to fail' }
    catch {
        if ($_.Exception.Message -notlike ('*' + $Expected + '*')) { throw }
    }
}
Reset-Fixture
$plan = (. $restartScript -Mode Plan | ConvertFrom-Json)
if (@($plan.pids).Count -ne 1 -or $plan.pids[0] -ne 10 -or $script:closed -or $script:launched) { throw 'Plan selected a CLI or changed process state' }
Reset-Fixture
$result = (. $restartScript -Mode Restart | ConvertFrom-Json)
if ($result.action -ne 'restarted' -or $result.desktop_pid -ne 42 -or -not $script:closed -or -not $script:launched) { throw 'Restart did not exit before activating the new desktop' }
Reset-Fixture; $script:missing = $true
Expect-Failure 'not installed'
Reset-Fixture; $script:noWindow = $true
Expect-Failure 'without an accessible window'
if ($script:launched) { throw 'Launched despite inability to close the old desktop' }
Reset-Fixture; $script:exitTimeout = $true
Expect-Failure 'did not exit'
if ($script:launched) { throw 'Launched despite exit timeout' }
Reset-Fixture; $script:launchTimeout = $true
Expect-Failure 'did not open'
Write-Output 'Desktop lifecycle fixtures passed (selection, plan, close-before-launch, missing package, no window, exit timeout, launch timeout).'
