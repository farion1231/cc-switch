param(
    [ValidateSet('Plan', 'Status', 'Restart')][string]$Mode = 'Restart',
    [ValidateRange(0, 20)][int]$ExitTimeoutSeconds = 3,
    [ValidateRange(0, 10)][int]$StopTimeoutSeconds = 10,
    [ValidateRange(0, 25)][int]$StartTimeoutSeconds = 25
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
trap {
    # Preserve exceptions for callers that source this script (including the
    # offline lifecycle fixtures); executable invocations return a short error.
    if ($MyInvocation.InvocationName -eq '.') { throw $_ }
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

# Resolve the installed package rather than matching a process name. Newer Codex
# releases use ChatGPT.exe; ordinary Codex CLI processes must remain untouched.
$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if (-not $package) {
    if ($Mode -eq 'Status') {
        @{ supported = $true; installed = $false; running = $false } | ConvertTo-Json -Compress
        return
    }
    throw 'Codex desktop (OpenAI.Codex) is not installed for this Windows user.'
}
$manifest = Get-AppxPackageManifest -Package $package.PackageFullName
$application = @($manifest.Package.Applications.Application) | Where-Object {
    [IO.Path]::GetFileName([string]$_.Executable) -in @('Codex.exe', 'ChatGPT.exe')
} | Select-Object -First 1
if (-not $application) {
    if ($Mode -eq 'Status') {
        @{ supported = $true; installed = $false; running = $false } | ConvertTo-Json -Compress
        return
    }
    throw 'Cannot identify the Codex desktop executable in the installed package.'
}
$executable = [IO.Path]::GetFullPath((Join-Path $package.InstallLocation ([string]$application.Executable)))
$packageRoot = [IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\') + '\'
if (-not $executable.StartsWith($packageRoot, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    if ($Mode -eq 'Status') {
        @{ supported = $true; installed = $false; running = $false } | ConvertTo-Json -Compress
        return
    }
    throw 'The Codex desktop executable is outside its installed package or unavailable.'
}
$appId = $package.PackageFamilyName + '!' + [string]$application.Id
function Get-DesktopProcesses {
    @(Get-Process -Name Codex, ChatGPT -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.Equals($executable, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
    })
}
function Get-DesktopDescendants($Roots) {
    # Capture ancestry and creation times, never command lines or credentials.
    # Hold process handles so PID reuse cannot redirect a later Kill().
    $rootTimes = @{}
    foreach ($root in $Roots) {
        try {
            $root.Refresh()
            if ($root.HasExited) { continue }
            if (-not $root.Path.Equals($executable, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Cannot stop the Codex desktop: its process identity changed.'
            }
            $null = $root.SafeHandle
            $rootTimes[$root.Id] = $root.StartTime.ToUniversalTime()
        } catch {
            if ($root.HasExited) { continue }
            throw 'Cannot stop the Codex desktop: its process identity could not be verified.'
        }
    }
    if ($rootTimes.Count -eq 0) { return }
    $snapshot = @(Get-CimInstance -ClassName Win32_Process -Property ProcessId, ParentProcessId, CreationDate)
    $children = @{}
    $queue = [Collections.Generic.Queue[object]]::new()
    foreach ($row in $snapshot) {
        $processId = [int]$row.ProcessId
        if ($rootTimes.ContainsKey($processId) -and $row.CreationDate -and
            [Math]::Abs((([DateTime]$row.CreationDate).ToUniversalTime() - $rootTimes[$processId]).TotalMilliseconds) -lt 1) {
            $queue.Enqueue(@{ ProcessId = $processId; Created = $rootTimes[$processId]; Depth = 0 })
        }
        $parentId = [int]$row.ParentProcessId
        if (-not $children.ContainsKey($parentId)) { $children[$parentId] = [Collections.Generic.List[object]]::new() }
        $children[$parentId].Add($row)
    }
    $seen = @{}
    while ($queue.Count -gt 0) {
        $parent = $queue.Dequeue()
        if ($seen.ContainsKey($parent.ProcessId)) { continue }
        $seen[$parent.ProcessId] = $true
        foreach ($row in $children[$parent.ProcessId]) {
            if (-not $row.CreationDate) { continue }
            $created = ([DateTime]$row.CreationDate).ToUniversalTime()
            if ($created -lt $parent.Created) { continue }
            $process = Get-Process -Id ([int]$row.ProcessId) -ErrorAction SilentlyContinue
            if (-not $process) { continue }
            try {
                $null = $process.SafeHandle
                if ($process.HasExited -or [Math]::Abs(($process.StartTime.ToUniversalTime() - $created).TotalMilliseconds) -ge 1) { continue }
                $queue.Enqueue(@{ ProcessId = $process.Id; Created = $created; Depth = $parent.Depth + 1 })
                if (-not $rootTimes.ContainsKey($process.Id)) {
                    [pscustomobject]@{ Process = $process; Created = $created; Depth = $parent.Depth + 1 }
                }
            } catch {
                if ($process.HasExited) { continue }
                throw 'Cannot inspect a Codex desktop child process safely. Quit the desktop and retry.'
            }
        }
    }
}
function Get-LiveDesktopDescendants($Descendants) {
    foreach ($entry in $Descendants) {
        $entry.Process.Refresh()
        if (-not $entry.Process.HasExited) { $entry }
    }
}
$before = @(Get-DesktopProcesses)
if ($Mode -eq 'Status') {
    @{ supported = $true; installed = $true; running = ($before.Count -gt 0) } | ConvertTo-Json -Compress
    return
}
if ($Mode -eq 'Plan') {
    @{ app_id = $appId; executable = $executable; pids = @($before | ForEach-Object Id); windows = @($before | Where-Object MainWindowHandle -NE 0 | ForEach-Object Id) } | ConvertTo-Json -Compress
    return
}
if ($before.Count -gt 0) {
    $descendants = @(Get-DesktopDescendants $before)
    $windows = @($before | Where-Object MainWindowHandle -NE 0)
    foreach ($process in $windows) {
        # Electron may keep the application alive after its last window closes.
        # A failed close request also falls through to the explicit restart below.
        try {
            $process.Refresh()
            if (-not $process.HasExited) { [void]$process.CloseMainWindow() }
        } catch {
            # A racing exit or rejected window close is handled by the fresh
            # process/descendant snapshot and explicit stop below.
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds($ExitTimeoutSeconds)
    while ((@(Get-DesktopProcesses).Count -gt 0 -or @(Get-LiveDesktopDescendants $descendants).Count -gt 0) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 200
    }
    # The user explicitly requested a desktop restart, including interruption
    # of its running tasks. Stop background-resident Electron processes only
    # after verifying their exact executable path against the installed package.
    $windowIds = @($windows | ForEach-Object Id)
    $remaining = @(Get-DesktopProcesses | Sort-Object @{ Expression = { $windowIds -notcontains $_.Id } }, Id)
    # Include descendants created during graceful shutdown, while retaining
    # handles to children whose parent has already exited.
    if ($remaining.Count -gt 0) {
        $descendants = @($descendants) + @(Get-DesktopDescendants $remaining)
    }
    $descendants = @($descendants | Sort-Object @{ Expression = { $_.Process.Id } }, Created -Unique)
    foreach ($entry in @($descendants | Sort-Object Depth -Descending)) {
        $process = $entry.Process
        try {
            $process.Refresh()
            if ($process.HasExited) { continue }
            if ([Math]::Abs(($process.StartTime.ToUniversalTime() - $entry.Created).TotalMilliseconds) -ge 1) {
                throw 'The desktop child process identity changed.'
            }
            $process.Kill()
        } catch {
            if ($process.HasExited) { continue }
            throw 'Cannot stop a Codex desktop child process. Quit the desktop and retry.'
        }
    }
    foreach ($process in $remaining) {
        try {
            $process.Refresh()
            if ($process.HasExited) { continue }
            if (-not $process.Path.Equals($executable, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'The desktop process identity changed while restarting.'
            }
            $process.Kill()
        } catch {
            # Another desktop process may have already exited with its parent.
            if ($process.HasExited) { continue }
            throw 'Cannot stop the Codex desktop process. Quit it from its tray menu and retry.'
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds($StopTimeoutSeconds)
    while (@(Get-DesktopProcesses).Count -gt 0 -or @(Get-LiveDesktopDescendants $descendants).Count -gt 0) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Codex desktop processes are still exiting. A second instance was not started. Quit it from its tray menu and retry.' }
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
