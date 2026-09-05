"""Windows regression: close nested launcher Jobs and verify shell-launched survival.

Run with Python on an interactive Windows desktop. Only synthetic probe
processes are launched/terminated; this does not start or stop CC Switch.
"""

import ctypes as c
import json
import os
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes as w
from pathlib import Path

script = Path(__file__).resolve()
repo = script.parents[2]
if os.name != "nt":
    raise SystemExit("This integration test requires an interactive Windows desktop")
if c.sizeof(c.c_void_p) != 8:
    raise SystemExit("This integration test requires 64-bit Python")
if len(sys.argv) > 1 and sys.argv[1] == "probe":
    Path(sys.argv[2]).write_text(str(os.getpid()))
    time.sleep(45)
    raise SystemExit()
if len(sys.argv) > 1 and sys.argv[1] == "host":
    folder = Path(sys.argv[2])
    info = subprocess.STARTUPINFO()
    info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    info.wShowWindow = 0
    old = subprocess.Popen(
        [sys.executable, str(script), "probe", str(folder / "old.txt")],
        startupinfo=info,
        creationflags=subprocess.CREATE_BREAKAWAY_FROM_JOB
        | subprocess.DETACHED_PROCESS,
    )
    args = subprocess.list2cmdline([str(script), "probe", str(folder / "explorer.txt")])
    result = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-File",
            str(repo / "scripts/launch-windows.ps1"),
            "-ExecutablePath",
            str(Path(sys.executable).with_name("pythonw.exe")),
            "-Arguments",
            args,
        ],
        capture_output=True,
        check=False,
        timeout=15,
    )
    if result.returncode:
        (folder / "error.txt").write_bytes(result.stderr)
        raise SystemExit(result.returncode)
    (folder / "host-ready.txt").write_text(
        json.dumps({"host": os.getpid(), "old": old.pid})
    )
    time.sleep(45)
    raise SystemExit()

k = c.WinDLL("kernel32", use_last_error=True)


class Startup(c.Structure):
    _fields_ = [
        ("cb", w.DWORD),
        ("reserved", w.LPWSTR),
        ("desktop", w.LPWSTR),
        ("title", w.LPWSTR),
        ("x", w.DWORD),
        ("y", w.DWORD),
        ("xs", w.DWORD),
        ("ys", w.DWORD),
        ("xc", w.DWORD),
        ("yc", w.DWORD),
        ("fill", w.DWORD),
        ("flags", w.DWORD),
        ("show", w.WORD),
        ("size", w.WORD),
        ("data", c.POINTER(c.c_byte)),
        ("stdin", w.HANDLE),
        ("stdout", w.HANDLE),
        ("stderr", w.HANDLE),
    ]


class Process(c.Structure):
    _fields_ = [
        ("process", w.HANDLE),
        ("thread", w.HANDLE),
        ("pid", w.DWORD),
        ("tid", w.DWORD),
    ]


k.CreateJobObjectW.argtypes = [w.LPVOID, w.LPCWSTR]
k.CreateJobObjectW.restype = w.HANDLE
k.SetInformationJobObject.argtypes = [w.HANDLE, c.c_int, w.LPVOID, w.DWORD]
k.AssignProcessToJobObject.argtypes = [w.HANDLE, w.HANDLE]
k.CreateProcessW.argtypes = [
    w.LPCWSTR,
    w.LPWSTR,
    w.LPVOID,
    w.LPVOID,
    w.BOOL,
    w.DWORD,
    w.LPVOID,
    w.LPCWSTR,
    c.POINTER(Startup),
    c.POINTER(Process),
]
k.ResumeThread.argtypes = [w.HANDLE]
k.CloseHandle.argtypes = [w.HANDLE]
k.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k.OpenProcess.restype = w.HANDLE
k.WaitForSingleObject.argtypes = [w.HANDLE, w.DWORD]
k.TerminateProcess.argtypes = [w.HANDLE, w.UINT]
k.IsProcessInJob.argtypes = [w.HANDLE, w.HANDLE, c.POINTER(w.BOOL)]


def check(ok):
    if not ok:
        raise c.WinError(c.get_last_error())


def make_job(flags):
    job = k.CreateJobObjectW(None, None)
    check(job)
    limits = c.create_string_buffer(144)
    limits[16:20] = flags.to_bytes(4, "little")
    check(k.SetInformationJobObject(job, 9, limits, len(limits)))
    return job


def member(process, job):
    result = w.BOOL()
    check(k.IsProcessInJob(process, job, c.byref(result)))
    return bool(result.value)


output_dir = repo / "src-tauri/target"
output_dir.mkdir(parents=True, exist_ok=True)
folder = Path(tempfile.mkdtemp(prefix="launcher-lifetime-", dir=output_dir))
outer = make_job(0x2000)
inner = make_job(0x2800)
pi = Process()
si = Startup()
si.cb = c.sizeof(si)
si.flags = 1
si.show = 0
old_handle = explorer_handle = None
try:
    command = c.create_unicode_buffer(
        subprocess.list2cmdline([sys.executable, str(script), "host", str(folder)])
    )
    check(
        k.CreateProcessW(
            sys.executable,
            command,
            None,
            None,
            False,
            0x08000004,
            None,
            str(repo),
            c.byref(si),
            c.byref(pi),
        )
    )
    check(k.AssignProcessToJobObject(outer, pi.process))
    check(k.AssignProcessToJobObject(inner, pi.process))
    k.ResumeThread(pi.thread)
    deadline = time.monotonic() + 20
    while not all(
        (folder / name).exists()
        for name in ("host-ready.txt", "old.txt", "explorer.txt")
    ):
        if (folder / "error.txt").exists():
            raise RuntimeError((folder / "error.txt").read_text(errors="replace"))
        if time.monotonic() > deadline:
            raise RuntimeError("Probe startup timed out")
        time.sleep(0.1)
    old = int((folder / "old.txt").read_text())
    explorer = int((folder / "explorer.txt").read_text())
    old_handle = k.OpenProcess(0x100000 | 0x1000 | 1, False, old)
    check(old_handle)
    explorer_handle = k.OpenProcess(0x100000 | 0x1000 | 1, False, explorer)
    check(explorer_handle)
    before = {
        "old_in_outer_job": member(old_handle, outer),
        "old_in_inner_job": member(old_handle, inner),
        "explorer_in_outer_job": member(explorer_handle, outer),
        "explorer_in_inner_job": member(explorer_handle, inner),
    }
    k.CloseHandle(inner)
    inner = None
    k.CloseHandle(outer)
    outer = None
    after = {
        "host_exited": k.WaitForSingleObject(pi.process, 3000) == 0,
        "old_exited": k.WaitForSingleObject(old_handle, 3000) == 0,
        "explorer_survived": k.WaitForSingleObject(explorer_handle, 1000) == 258,
    }
    assert before == {
        "old_in_outer_job": True,
        "old_in_inner_job": False,
        "explorer_in_outer_job": False,
        "explorer_in_inner_job": False,
    }, before
    assert all(after.values()), after
    print(
        json.dumps(
            {
                "before_host_exit": before,
                "after_host_exit": after,
                "probe_pids": {"old": old, "explorer": explorer},
            }
        )
    )
finally:
    if inner:
        k.CloseHandle(inner)
    if outer:
        k.CloseHandle(outer)
    for handle in (old_handle, explorer_handle, pi.process):
        if handle:
            if k.WaitForSingleObject(handle, 0) == 258:
                k.TerminateProcess(handle, 0)
            k.CloseHandle(handle)
    if pi.thread:
        k.CloseHandle(pi.thread)
