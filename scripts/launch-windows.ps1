<#
.SYNOPSIS
Launch CC Switch through the existing desktop shell, outside a terminal/tool Job.
.DESCRIPTION
CREATE_BREAKAWAY_FROM_JOB alone can leave a process in an outer nested Job.
Use the running Explorer desktop as the launch broker instead. No fallback to
Start-Process is attempted if the interactive desktop shell is unavailable.
#>
[CmdletBinding()]
param(
    [string]$ExecutablePath = (Join-Path $env:LOCALAPPDATA 'CC Switch\cc-switch.exe'),
    [string]$Arguments = ''
)

$ErrorActionPreference = 'Stop'
$executable = (Get-Item -LiteralPath $ExecutablePath).FullName
if ([IO.Path]::GetExtension($executable) -ine '.exe') {
    throw 'ExecutablePath must point to a Windows executable.'
}

$shell = New-Object -ComObject Shell.Application
$desktopHwnd = 0
# SWC_DESKTOP = 8; SWFO_NEEDDISPATCH = 1. Resolve the existing desktop rather
# than creating a new Explorer process beneath the caller.
$desktop = $shell.Windows().FindWindowSW(0, 0, 8, [ref]$desktopHwnd, 1)
if ($null -eq $desktop) {
    throw 'The interactive Explorer desktop is unavailable; CC Switch was not launched.'
}
$desktop.Document.Application.ShellExecute(
    $executable, $Arguments, [IO.Path]::GetDirectoryName($executable), 'open', 0
)
Write-Output 'Launch requested through the Explorer desktop.'
