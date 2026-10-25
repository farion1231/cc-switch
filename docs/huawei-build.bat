@echo off
REM ============================================================
REM  CC-Switch Huawei CodeAgent Build Script
REM  Usage: cd src-tauri && build.bat
REM ============================================================

echo ============================================================
echo  CC-Switch Huawei CodeAgent Build Script
echo ============================================================
echo.

REM Step 1: Initialize MSVC x64 build environment
echo [1/5] Initializing MSVC x64 build environment...
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64
if %ERRORLEVEL% neq 0 (
    echo ERROR: Failed to initialize MSVC environment
    echo Make sure Visual Studio Build Tools 2022 is installed with C++ workload
    pause
    exit /b 1
)
echo OK: MSVC environment initialized
echo.

REM Step 2: Verify tools
echo [2/5] Verifying build tools...
where cl.exe >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERROR: cl.exe not found. MSVC environment not initialized.
    pause
    exit /b 1
)
echo OK: cl.exe found
where link.exe >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERROR: link.exe not found. MSVC environment not initialized.
    pause
    exit /b 1
)
echo OK: link.exe found
echo.

REM Step 3: Set mirror source (for China mainland)
echo [3/5] Setting up mirror sources...
set NO_PROXY=mirrors.ustc.edu.cn,*.huawei.com
set no_proxy=mirrors.ustc.edu.cn,*.huawei.com
set RUSTUP_DIST_SERVER=https://rust.inhuawei.com/rustup-static
echo OK: Mirror sources configured
echo.

REM Step 4: Build release binary
echo [4/5] Building release binary (this may take 10-30 minutes)...
echo.

REM Check if Rust toolchain file exists and adjust if needed
if exist ..\rust-toolchain.toml (
    echo Found rust-toolchain.toml, ensuring correct version...
    rustup show active-toolchain 2>nul
    if %ERRORLEVEL% neq 0 (
        echo WARNING: No active toolchain found. Attempting to install 1.95.0...
        rustup toolchain install 1.95.0-x86_64-pc-windows-msvc --profile minimal
    )
)

echo Starting cargo build --release...
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo.
    echo ============================================================
    echo  BUILD FAILED with exit code %ERRORLEVEL%
    echo ============================================================
    echo.
    echo Common causes:
    echo   - C1056 error: Disable antivirus real-time protection
    echo   - Network error: Configure cargo mirror source
    echo   - link.exe not found: Run vcvarsall.bat first
    echo.
    pause
    exit /b 1
)

REM Step 5: Verify build result
echo.
echo [5/5] Verifying build result...
if exist target\release\cc-switch.exe (
    echo ============================================================
    echo  BUILD SUCCESSFUL!
    echo ============================================================
    echo.
    dir target\release\cc-switch.exe | findstr "cc-switch.exe"
    echo.
    echo Binary location: %CD%\target\release\cc-switch.exe
    echo.
    echo You can now run: target\release\cc-switch.exe
) else (
    echo WARNING: cc-switch.exe not found in target\release\
    echo Check build output above for errors.
)
echo.
pause
