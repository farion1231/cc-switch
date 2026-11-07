# Offline lifecycle fixtures. No real package, process, window, or launch is used.
$ErrorActionPreference = 'Stop'
$restartScript = Join-Path $PSScriptRoot '../../src-tauri/src/services/codex_desktop_windows.ps1'
$script:exe = 'C:\Program Files\WindowsApps\OpenAI.Codex_test\app\ChatGPT.exe'
$script:rootTime = [DateTime]::UtcNow.AddMinutes(-1)
function Get-CimInstance {
    param($ClassName, $Property)
    [pscustomobject]@{ ProcessId = 10; ParentProcessId = 1; CreationDate = $script:rootTime }
    if ($script:includeChild) {
        [pscustomobject]@{ ProcessId = 20; ParentProcessId = 10; CreationDate = $script:rootTime.AddSeconds(1) }
    }
    if ($script:grandchildBorn) {
        [pscustomobject]@{ ProcessId = 30; ParentProcessId = 20; CreationDate = $script:grandchildTime }
    }
}
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
    param($Name, $Id, $ErrorAction)
    if ($PSBoundParameters.ContainsKey('Id')) {
        if ($Id -eq 30 -and $script:grandchildBorn -and -not $script:grandchildKilled) {
            $grandchild = [pscustomobject]@{ Id = 30; Path = 'C:\tools\tool.exe'; StartTime = $script:grandchildTime; ExitTime = $script:rootTime.AddSeconds(20); SafeHandle = 1; HasExited = $false }
            $grandchild | Add-Member -MemberType ScriptMethod -Name Refresh -Value { $this.HasExited = $script:grandchildKilled }
            $grandchild | Add-Member -MemberType ScriptMethod -Name Kill -Value {
                if ($script:grandchildStopDenied) { throw 'Access denied' }
                $script:grandchildKilled = $true
            }
            return $grandchild
        }
        if ($Id -ne 20 -or -not $script:includeChild -or $script:childKilled) { return }
        $child = [pscustomobject]@{ Id = 20; Path = 'C:\tools\Codex\bin\codex.exe'; StartTime = $script:rootTime.AddSeconds($(if ($script:childPidReused) { 5 } else { 1 })); ExitTime = $script:rootTime.AddSeconds(10); SafeHandle = 1; HasExited = $false }
        $child | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
            $this.HasExited = $script:childKilled
            if ($script:childIdentityChanged) { $this.StartTime = $script:rootTime.AddSeconds(3) }
        }
        $child | Add-Member -MemberType ScriptMethod -Name Kill -Value {
            if ($script:childStopDenied) { throw 'Access denied' }
            if ($script:spawnDuringStop) { $script:grandchildBorn = $true }
            $script:childKilled = $true
        }
        return $child
    }
    if ($script:launched) {
        if ($script:launchTimeout) { return }
        return [pscustomobject]@{ Id = 42; Path = $script:exe; MainWindowHandle = 100 }
    }
    if ($script:noDesktop) {
        # A separately installed CLI cannot make the desktop appear running.
        [pscustomobject]@{ Id = 99; Path = 'C:\tools\codex.exe'; MainWindowHandle = 200 }
        return
    }
    if ($script:killed -and -not $script:stopTimeout) { return }
    if ($script:closed -and -not $script:backgroundResident) { return }
    $desktop = [pscustomobject]@{ Id = 10; Path = $script:exe; StartTime = $script:rootTime; ExitTime = $script:rootTime.AddSeconds(10); SafeHandle = 1; MainWindowHandle = $(if ($script:noWindow) { 0 } else { 100 }) }
    $desktop | Add-Member -NotePropertyName HasExited -NotePropertyValue $false
    $desktop | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
        if ($script:identityChanged) { $this.Path = 'C:\tools\codex.exe' }
        $this.HasExited = ($script:killed -and -not $script:stopTimeout) -or ($script:closed -and -not $script:backgroundResident)
    }
    $desktop | Add-Member -MemberType ScriptMethod -Name CloseMainWindow -Value {
        $script:closed = $true
        if ($script:spawnDuringClose) { $script:grandchildBorn = $true }
        (-not $script:closeRejected)
    }
    $desktop | Add-Member -MemberType ScriptMethod -Name Kill -Value {
        if ($script:stopDenied) { throw 'Access denied' }
        $script:killed = $true
    }
    $desktop
    # A CLI with the same process name must never be included in the restart.
    [pscustomobject]@{ Id = 99; Path = 'C:\tools\codex.exe'; MainWindowHandle = 200 }
}
function Start-Process {
    param($FilePath, $ArgumentList, $WindowStyle)
    if ($FilePath -ne 'explorer.exe' -or $ArgumentList -ne 'shell:AppsFolder\OpenAI.Codex_test!App') { throw 'Unexpected desktop activation' }
    if ($script:backgroundResident -and -not $script:killed) { throw 'Activated before background desktop exited' }
    if ($script:includeChild -and -not $script:childPidReused -and -not $script:childKilled) { throw 'Activated before desktop child exited' }
    if ($script:grandchildBorn -and -not $script:grandchildKilled -and -not $script:unrelatedGrandchild) { throw 'Activated before late desktop grandchild exited' }
    $script:launched = $true
}
function Start-Sleep { param($Milliseconds) }
function Reset-Fixture {
    $script:missing = $false; $script:closed = $false; $script:launched = $false
    $script:noWindow = $false; $script:backgroundResident = $false; $script:launchTimeout = $false
    $script:noDesktop = $false
    $script:includeChild = $false; $script:childKilled = $false; $script:childStopDenied = $false; $script:childIdentityChanged = $false; $script:childPidReused = $false
    $script:grandchildBorn = $false; $script:grandchildKilled = $false; $script:grandchildStopDenied = $false; $script:spawnDuringClose = $false; $script:spawnDuringStop = $false
    $script:grandchildTime = $script:rootTime.AddSeconds(2); $script:unrelatedGrandchild = $false
    $script:killed = $false; $script:stopDenied = $false; $script:stopTimeout = $false; $script:closeRejected = $false; $script:identityChanged = $false
}
function Expect-Failure([string]$Expected) {
    try { . $restartScript -ExitTimeoutSeconds 0 -StopTimeoutSeconds 0 -StartTimeoutSeconds 0; throw 'Expected restart to fail' }
    catch {
        if ($_.Exception.Message -notlike ('*' + $Expected + '*')) { throw }
    }
}
Reset-Fixture
$status = (. $restartScript -Mode Status | ConvertFrom-Json)
if (-not $status.supported -or -not $status.installed -or -not $status.running -or $script:closed -or $script:launched) { throw 'Running desktop status mutated process state' }
Reset-Fixture; $script:noWindow = $true
$status = (. $restartScript -Mode Status | ConvertFrom-Json)
if (-not $status.running) { throw 'Background desktop was reported as stopped' }
Reset-Fixture; $script:noDesktop = $true
$status = (. $restartScript -Mode Status | ConvertFrom-Json)
if (-not $status.installed -or $status.running) { throw 'CLI was reported as a running desktop' }
Reset-Fixture; $script:missing = $true
$status = (. $restartScript -Mode Status | ConvertFrom-Json)
if (-not $status.supported -or $status.installed -or $status.running) { throw 'Missing desktop status was incorrect' }
Reset-Fixture; $script:noDesktop = $true
$result = (. $restartScript -Mode Restart | ConvertFrom-Json)
if ($result.action -ne 'started' -or $result.desktop_pid -ne 42 -or $script:closed -or $script:killed) { throw 'Opening a stopped desktop attempted to stop a process' }
Reset-Fixture
$plan = (. $restartScript -Mode Plan | ConvertFrom-Json)
if (@($plan.pids).Count -ne 1 -or $plan.pids[0] -ne 10 -or $script:closed -or $script:launched) { throw 'Plan selected a CLI or changed process state' }
Reset-Fixture
$result = (. $restartScript -Mode Restart | ConvertFrom-Json)
if ($result.action -ne 'restarted' -or $result.desktop_pid -ne 42 -or -not $script:closed -or -not $script:launched -or $script:killed) { throw 'Graceful restart did not exit before activating the new desktop' }
Reset-Fixture; $script:missing = $true
Expect-Failure 'not installed'
Reset-Fixture; $script:backgroundResident = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:killed -or -not $script:launched -or $result.desktop_pid -ne 42) { throw 'Background-resident desktop was not restarted' }
Reset-Fixture; $script:noWindow = $true; $script:backgroundResident = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if ($script:closed -or -not $script:killed -or $result.desktop_pid -ne 42) { throw 'Windowless desktop was not restarted' }
Reset-Fixture; $script:closeRejected = $true; $script:backgroundResident = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:killed -or $result.desktop_pid -ne 42) { throw 'Rejected close request prevented explicit restart' }
Reset-Fixture; $script:stopDenied = $true; $script:backgroundResident = $true
Expect-Failure 'Cannot stop'
if ($script:launched) { throw 'Launched despite stop failure' }
Reset-Fixture; $script:identityChanged = $true; $script:backgroundResident = $true
Expect-Failure 'Cannot stop'
if ($script:killed -or $script:launched) { throw 'Stopped a process after its executable identity changed' }
Reset-Fixture; $script:stopTimeout = $true; $script:backgroundResident = $true
Expect-Failure 'still exiting'
if ($script:launched) { throw 'Launched while old processes were still exiting' }
Reset-Fixture; $script:launchTimeout = $true
Expect-Failure 'did not open'
Reset-Fixture; $script:includeChild = $true; $script:backgroundResident = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:childKilled -or -not $script:killed -or $result.desktop_pid -ne 42) { throw 'Desktop child outside the package survived restart' }
Reset-Fixture; $script:includeChild = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:childKilled -or $script:killed -or $result.desktop_pid -ne 42) { throw 'Child orphaned by graceful parent exit survived restart' }
Reset-Fixture; $script:includeChild = $true; $script:childStopDenied = $true; $script:backgroundResident = $true
Expect-Failure 'Cannot stop a Codex desktop child'
if ($script:launched) { throw 'Launched despite child stop failure' }
Reset-Fixture; $script:includeChild = $true; $script:childIdentityChanged = $true; $script:backgroundResident = $true
Expect-Failure 'Cannot stop a Codex desktop child'
if ($script:childKilled -or $script:launched) { throw 'Stopped child after its identity changed' }
Reset-Fixture; $script:includeChild = $true; $script:childPidReused = $true; $script:backgroundResident = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if ($script:childKilled -or $result.desktop_pid -ne 42) { throw 'Stopped a reused child PID' }
Reset-Fixture; $script:includeChild = $true; $script:spawnDuringClose = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:grandchildKilled -or $script:killed -or $result.desktop_pid -ne 42) { throw 'Grandchild created during graceful root exit survived restart' }
Reset-Fixture; $script:includeChild = $true; $script:spawnDuringStop = $true
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if (-not $script:grandchildKilled -or $result.desktop_pid -ne 42) { throw 'Grandchild created during child termination survived restart' }
Reset-Fixture; $script:includeChild = $true; $script:spawnDuringClose = $true; $script:grandchildStopDenied = $true
Expect-Failure 'Cannot stop a Codex desktop child'
if ($script:launched) { throw 'Launched despite late grandchild stop failure' }
Reset-Fixture; $script:includeChild = $true; $script:spawnDuringStop = $true
Expect-Failure 'still exiting'
if ($script:launched) { throw 'Launched before late descendants converged within the stop deadline' }
Reset-Fixture; $script:includeChild = $true; $script:spawnDuringStop = $true; $script:unrelatedGrandchild = $true
$script:grandchildTime = $script:rootTime.AddSeconds(11)
$result = (. $restartScript -ExitTimeoutSeconds 0 | ConvertFrom-Json)
if ($script:grandchildKilled -or $result.desktop_pid -ne 42) { throw 'Stopped a child born after the tracked parent exited (reused parent PID)' }
Write-Output 'Desktop lifecycle fixtures passed (status, selection, graceful exit, background residency, late descendants, orphaned child, PID reuse, identity change, stop denied/timeouts, launch timeout).'
