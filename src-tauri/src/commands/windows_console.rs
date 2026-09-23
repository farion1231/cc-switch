use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_GREATER_THAN, CSTR_LESS_THAN};
use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTUPINFOW,
};

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn append_windows_arg(line: &mut Vec<u16>, value: &OsStr, force_quotes: bool) -> io::Result<()> {
    let units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(invalid_input("Windows command argument contains NUL"));
    }
    let quote = force_quotes
        || units.is_empty()
        || units
            .iter()
            .any(|unit| matches!(*unit, 9 | 10 | 11 | 12 | 13 | 32 | 34));
    if !quote {
        line.extend(units);
        return Ok(());
    }

    line.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in units {
        if unit == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        line.extend(std::iter::repeat_n(
            b'\\' as u16,
            backslashes * if unit == b'"' as u16 { 2 } else { 1 },
        ));
        backslashes = 0;
        if unit == b'"' as u16 {
            line.push(b'\\' as u16);
        }
        line.push(unit);
    }
    line.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    line.push(b'"' as u16);
    Ok(())
}

fn system_directory() -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if (length as usize) < buffer.len() {
            buffer.truncate(length as usize);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

fn console_executable(program: &OsStr) -> io::Result<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let relative = match program.to_str() {
        Some(name) if name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("cmd.exe") => {
            "cmd.exe"
        }
        Some(name)
            if name.eq_ignore_ascii_case("powershell")
                || name.eq_ignore_ascii_case("powershell.exe") =>
        {
            "WindowsPowerShell/v1.0/powershell.exe"
        }
        _ => {
            return Err(invalid_input(
                "new console requires a system shell or absolute executable",
            ))
        }
    };
    Ok(system_directory()?.join(relative))
}

fn command_line(command: &Command, executable: &Path) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    append_windows_arg(&mut line, executable.as_os_str(), true)?;
    for arg in command.get_args() {
        line.push(b' ' as u16);
        append_windows_arg(&mut line, arg, false)?;
    }
    line.push(0);
    Ok(line)
}

fn environment_name(entry: &[u16]) -> &[u16] {
    // Windows' hidden drive entries start with `=`, so their separator is the
    // *second* equals sign. Preserve them when copying the parent environment.
    let separator = entry
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, unit)| **unit == b'=' as u16)
        .map(|(index, _)| index)
        .unwrap_or(entry.len());
    &entry[..separator]
}

fn compare_environment_names(left: &[u16], right: &[u16]) -> Ordering {
    match unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_GREATER_THAN => Ordering::Greater,
        2 => Ordering::Equal,
        _ => left.cmp(right),
    }
}

fn environment_block(command: &Command) -> io::Result<Vec<u16>> {
    let overrides: Vec<_> = command
        .get_envs()
        .map(|(name, value)| {
            let name: Vec<u16> = name.encode_wide().collect();
            if name.is_empty() || name.contains(&(b'=' as u16)) || name.contains(&0) {
                return Err(invalid_input("invalid Windows environment variable name"));
            }
            let value = value.map(|value| value.encode_wide().collect::<Vec<_>>());
            if value.as_ref().is_some_and(|value| value.contains(&0)) {
                return Err(invalid_input("Windows environment value contains NUL"));
            }
            Ok((name, value))
        })
        .collect::<io::Result<_>>()?;

    let parent = unsafe { GetEnvironmentStringsW() };
    if parent.is_null() {
        return Err(io::Error::last_os_error());
    }
    struct ParentEnvironment(*mut u16);
    impl Drop for ParentEnvironment {
        fn drop(&mut self) {
            unsafe { FreeEnvironmentStringsW(self.0) };
        }
    }
    let _parent = ParentEnvironment(parent);

    let mut entries = Vec::new();
    let mut cursor = parent;
    loop {
        let mut length = 0;
        // GetEnvironmentStringsW owns a double-NUL-terminated UTF-16 block;
        // keep it alive until every entry has been copied into our own storage.
        while unsafe { *cursor.add(length) } != 0 {
            length += 1;
        }
        if length == 0 {
            break;
        }
        let entry = unsafe { std::slice::from_raw_parts(cursor, length) };
        if !overrides.iter().any(|(name, _)| {
            compare_environment_names(environment_name(entry), name) == Ordering::Equal
        }) {
            entries.push(entry.to_vec());
        }
        cursor = unsafe { cursor.add(length + 1) };
    }
    for (name, value) in overrides {
        if let Some(value) = value {
            let mut entry = name;
            entry.push(b'=' as u16);
            entry.extend(value);
            entries.push(entry);
        }
    }
    entries.sort_by(|left, right| {
        compare_environment_names(environment_name(left), environment_name(right))
    });
    let mut block = Vec::new();
    for entry in entries {
        block.extend(entry);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

/// Unlike `Command::spawn`, leave STARTF_USESTDHANDLES unset so Windows connects
/// all three standard streams to the new console, even if CC Switch was itself
/// launched with redirected pipes. The inherited environment is copied and the
/// command's per-launch additions/removals are applied without losing Unicode.
/// Specify the executable explicitly: CreateProcessW with a null application
/// name would search the parent's working directory before the system folder.
pub(super) fn spawn_new_console(command: &mut Command) -> io::Result<()> {
    let executable = console_executable(command.get_program())?;
    let mut application: Vec<u16> = executable.as_os_str().encode_wide().collect();
    if application.contains(&0) {
        return Err(invalid_input("Windows executable path contains NUL"));
    }
    application.push(0);
    let mut line = command_line(command, &executable)?;
    let environment = environment_block(command)?;
    let cwd = command
        .get_current_dir()
        .map(|path| {
            let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
            if value.contains(&0) {
                return Err(invalid_input("Windows working directory contains NUL"));
            }
            value.push(0);
            Ok(value)
        })
        .transpose()?;
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    #[test]
    fn new_console_keeps_unicode_paths_and_clears_parent_session() {
        use super::super::misc::{
            configure_windows_batch_env_with, WINDOWS_BATCH_PATH_COMMAND, WINDOWS_CONFIG_PATH_ENV,
            WINDOWS_CWD_ENV, WINDOWS_POWERSHELL_BATCH_COMMAND,
        };

        let temp = tempfile::tempdir().unwrap();
        let directory = temp
            .path()
            .join("迅雷下载 & %CC_SWITCH_CMD_EXPAND% ^ ! (test) O'Brien");
        std::fs::create_dir(&directory).unwrap();
        let cwd = directory.to_string_lossy();
        for shell in ["cmd", "powershell"] {
            let batch = directory.join(format!("{shell}.bat"));
            let marker = directory.join(format!("{shell} 配置结果.txt"));
            let cwd_marker = directory.join(format!("{shell}-cwd.txt"));
            let batch_content = format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"CC_SWITCH_INTERNAL_BATCH_PATH=\"\r\nif defined CLAUDECODE exit /b 5\r\nif defined NO_COLOR exit /b 6\r\nif defined NODE_DISABLE_COLORS exit /b 7\r\ncd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 8\r\n> \"%CC_SWITCH_INTERNAL_CONFIG_PATH%\" echo valid\r\n> \"{shell}-cwd.txt\" echo ok\r\n"
            );
            assert!(batch_content.is_ascii());
            std::fs::write(&batch, batch_content).unwrap();

            let mut command = Command::new(shell);
            if shell == "cmd" {
                command.args(["/D", "/V:OFF", "/C", WINDOWS_BATCH_PATH_COMMAND]);
            } else {
                command.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    WINDOWS_POWERSHELL_BATCH_COMMAND,
                ]);
            }
            command
                .env("CLAUDECODE", "stale-parent-session")
                .env("NO_COLOR", "1")
                .env("NODE_DISABLE_COLORS", "1")
                .env("CC_SWITCH_CMD_EXPAND", "wrong");
            let batch_path = batch.to_string_lossy();
            let config_path = marker.to_string_lossy();
            configure_windows_batch_env_with(
                &mut command,
                &batch_path,
                &[
                    (WINDOWS_CONFIG_PATH_ENV, &config_path),
                    (WINDOWS_CWD_ENV, &cwd),
                ],
            );
            spawn_new_console(&mut command).unwrap();

            let deadline = Instant::now() + Duration::from_secs(5);
            while (!marker.exists() || !cwd_marker.exists()) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                std::fs::read_to_string(&marker)
                    .unwrap_or_else(|error| panic!("{shell}: config marker missing: {error}"))
                    .trim(),
                "valid"
            );
            assert_eq!(std::fs::read_to_string(cwd_marker).unwrap().trim(), "ok");
        }
    }

    #[test]
    fn stdio_child_probe() {
        let Some(shell) = std::env::var_os("CC_SWITCH_CONSOLE_TEST_SHELL") else {
            return;
        };
        let script = "$streams = @([Console]::IsInputRedirected, [Console]::IsOutputRedirected, [Console]::IsErrorRedirected) -join ','; [IO.File]::WriteAllText('cc-switch-console-marker.txt', $streams); [Console]::Out.WriteLine('CC_SWITCH_CONSOLE_STDOUT'); [Console]::Error.WriteLine('CC_SWITCH_CONSOLE_STDERR')";
        let encoded = super::super::misc::powershell_encoded_command(script);
        let mut command = Command::new(&shell);
        command.current_dir(std::env::var_os("CC_SWITCH_CONSOLE_TEST_DIR").unwrap());
        if shell == "cmd" {
            let bootstrap = format!(
                "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}"
            );
            command.args(["/D", "/V:OFF", "/C", &bootstrap]);
        } else {
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &encoded,
            ]);
        }
        spawn_new_console(&mut command).expect("shell should launch");
    }

    #[test]
    fn new_console_does_not_search_parent_directory_for_system_shell() {
        let temp = tempfile::tempdir().unwrap();
        let system_root = std::env::var_os("SystemRoot").expect("Windows system root");
        let where_exe = std::path::Path::new(&system_root).join("System32/where.exe");
        std::fs::copy(where_exe, temp.path().join("cmd.exe")).unwrap();
        let marker = temp.path().join("cc-switch-console-marker.txt");
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::windows_console::tests::stdio_child_probe",
                "--nocapture",
            ])
            .current_dir(temp.path())
            .env_remove("NoDefaultCurrentDirectoryInExePath")
            .env("CC_SWITCH_CONSOLE_TEST_SHELL", "cmd")
            .env("CC_SWITCH_CONSOLE_TEST_DIR", temp.path())
            .output()
            .expect("probe parent should launch");
        assert!(output.status.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            marker.exists(),
            "an executable in the parent directory replaced system cmd.exe"
        );
    }

    #[test]
    fn new_console_does_not_inherit_redirected_standard_streams() {
        let temp = tempfile::tempdir().expect("probe directory");
        for shell in ["cmd", "powershell"] {
            let directory = temp.path().join(shell);
            std::fs::create_dir(&directory).unwrap();
            let marker = directory.join("cc-switch-console-marker.txt");
            let mut parent = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "commands::windows_console::tests::stdio_child_probe",
                    "--nocapture",
                ])
                .env("CC_SWITCH_CONSOLE_TEST_SHELL", shell)
                .env("CC_SWITCH_CONSOLE_TEST_DIR", &directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("redirected parent should launch");
            drop(parent.stdin.take());
            let output = parent.wait_with_output().expect("probe should finish");
            assert!(output.status.success(), "probe failed for {shell}");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                std::fs::read_to_string(&marker)
                    .unwrap_or_else(|error| panic!(
                        "{shell}: console probe did not execute: {error}"
                    ))
                    .trim(),
                "False,False,False",
                "{shell} must connect stdin, stdout and stderr to the new console"
            );
            assert!(
                !output
                    .stdout
                    .windows(b"CC_SWITCH_CONSOLE_STDOUT".len())
                    .any(|window| window == b"CC_SWITCH_CONSOLE_STDOUT")
                    && !output
                        .stderr
                        .windows(b"CC_SWITCH_CONSOLE_STDERR".len())
                        .any(|window| window == b"CC_SWITCH_CONSOLE_STDERR"),
                "{shell} inherited redirected stdout/stderr despite a new console"
            );
        }
    }
}
