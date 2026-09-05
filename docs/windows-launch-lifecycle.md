# Launching CC Switch from Windows tools

When a terminal, build runner or desktop coding agent owns a Windows Job with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, applications launched beneath it can be
terminated when that tool exits. `DETACHED_PROCESS` only detaches the console.
`CREATE_BREAKAWAY_FROM_JOB` can still leave the application inside an outer Job
that does not permit breakaway.

After installing or updating CC Switch from such a tool, use the existing
Explorer desktop to launch it:

```powershell
powershell.exe -NoProfile -File scripts/launch-windows.ps1
```

The default target is the per-user installation. Pass `-ExecutablePath` for a
different installation or build. The script requests a launch through the
desktop Shell COM object; it does not start a replacement Explorer beneath the
tool. An interactive desktop is required. If that desktop is unavailable, the
script reports an error instead of silently falling back to a dependent launch.
An already-running instance must be exited before changing its launch lifetime.
Normal launches from the desktop/start menu need no additional configuration.

The isolated Windows regression creates an outer kill-on-close Job and an inner
Job that permits breakaway, then closes both. The legacy breakaway-launched probe
dies; the Explorer-launched probe survives. It does not close CC Switch or the
coding agent:

```powershell
python scripts/tests/windows-launch-lifetime.py
```

References: [Windows nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs)
and [ShellWindows.FindWindowSW](https://learn.microsoft.com/en-us/windows/win32/api/exdisp/nf-exdisp-ishellwindows-findwindowsw).
