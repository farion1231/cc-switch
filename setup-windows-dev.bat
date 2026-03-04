@echo off
setlocal EnableExtensions EnableDelayedExpansion

title CC-Switch Windows Dev Setup

set "ROOT_DIR=%~dp0"
cd /d "%ROOT_DIR%"

set "RUN_DEV=0"
for %%A in (%*) do (
  if /I "%%~A"=="--run" set "RUN_DEV=1"
)

echo.
echo ============================================================
echo   CC-Switch one-click setup for Windows (Tauri + Rust + PNPM)
echo ============================================================
echo.

where winget >nul 2>&1
if errorlevel 1 (
  echo [ERROR] winget is not available.
  echo Install "App Installer" from Microsoft Store, then re-run this script.
  exit /b 1
)

call :ensure_admin "%~f0" "%*"
if errorlevel 2 exit /b 0
if errorlevel 1 exit /b 1

echo [STEP] Initializing winget sources...
winget source update --name winget --accept-source-agreements >nul 2>&1
winget source list --accept-source-agreements >nul 2>&1

echo [STEP] Checking required system packages...
call :ensure_winget_package "Git.Git"
if errorlevel 1 exit /b 1

call :ensure_winget_package "OpenJS.NodeJS.LTS"
if errorlevel 1 exit /b 1

call :ensure_winget_package "Rustlang.Rustup"
if errorlevel 1 exit /b 1

call :ensure_winget_package "Microsoft.EdgeWebView2Runtime"
if errorlevel 1 exit /b 1

call :ensure_vs_build_tools
if errorlevel 1 exit /b 1

echo [STEP] Refreshing PATH for current shell...
set "PATH=%PATH%;%ProgramFiles%\nodejs;%APPDATA%\npm;%USERPROFILE%\.cargo\bin"

echo [STEP] Verifying core tools...
call :require_command git
if errorlevel 1 exit /b 1
call :require_command node
if errorlevel 1 exit /b 1
call :require_command npm
if errorlevel 1 exit /b 1
call :require_command rustup
if errorlevel 1 exit /b 1
call :require_command cargo
if errorlevel 1 exit /b 1

echo [STEP] Enabling Corepack and PNPM...
call :run_step "corepack enable" "Enable Corepack"
if errorlevel 1 exit /b 1
call :run_step "corepack prepare pnpm@10.10.0 --activate" "Activate pnpm@10.10.0"
if errorlevel 1 (
  echo [WARN] Corepack pnpm activation failed. Falling back to npm global install...
  call :run_step "npm install -g pnpm@10.10.0" "Install pnpm globally"
  if errorlevel 1 exit /b 1
)

call :require_command pnpm
if errorlevel 1 exit /b 1

echo [STEP] Initializing Rust toolchain...
call :run_step "rustup default stable" "Set rust stable as default"
if errorlevel 1 exit /b 1
call :run_step "rustup target add x86_64-pc-windows-msvc" "Add MSVC target"
if errorlevel 1 exit /b 1

echo [STEP] Installing JS dependencies...
call :run_step "pnpm install --frozen-lockfile" "pnpm install (frozen-lockfile)"
if errorlevel 1 (
  echo [WARN] Frozen lockfile install failed. Retrying without --frozen-lockfile...
  call :run_step "pnpm install" "pnpm install"
  if errorlevel 1 exit /b 1
)

echo [STEP] Pre-fetching Rust crates...
call :run_step "cargo fetch --manifest-path src-tauri\Cargo.toml" "cargo fetch"
if errorlevel 1 exit /b 1

echo.
echo ============================================================
echo Setup completed successfully.
echo You can now run:
echo   pnpm dev
echo ============================================================
echo.

if "%RUN_DEV%"=="1" (
  echo [STEP] Starting dev server...
  pnpm dev
)

exit /b 0

:ensure_admin
set "SCRIPT_PATH=%~1"
set "ORIGINAL_ARGS=%~2"

net session >nul 2>&1
if %errorlevel%==0 (
  exit /b 0
)

echo [INFO] Requesting Administrator privileges...
set "ELEVATED_ARGS=--elevated"
if defined ORIGINAL_ARGS set "ELEVATED_ARGS=%ELEVATED_ARGS% %ORIGINAL_ARGS%"
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%SCRIPT_PATH%' -ArgumentList '%ELEVATED_ARGS%' -Verb RunAs"
if errorlevel 1 (
  echo [ERROR] Could not acquire Administrator privileges.
  exit /b 1
)
exit /b 2

:ensure_winget_package
set "PKG_ID=%~1"
echo [INFO] Checking %PKG_ID% ...
set "WINGET_CHECK_LOG=%TEMP%\ccswitch_winget_check_%RANDOM%.log"
winget list --id "%PKG_ID%" -e --source winget --accept-source-agreements --disable-interactivity >"%WINGET_CHECK_LOG%" 2>&1
findstr /I /C:"%PKG_ID%" "%WINGET_CHECK_LOG%" >nul
if not errorlevel 1 (
  echo [OK] %PKG_ID% is already installed.
  del /q "%WINGET_CHECK_LOG%" >nul 2>&1
  exit /b 0
)
del /q "%WINGET_CHECK_LOG%" >nul 2>&1

echo [INFO] Installing %PKG_ID% ...
winget install --id "%PKG_ID%" -e --source winget --disable-interactivity --accept-source-agreements --accept-package-agreements --silent
if errorlevel 1 (
  echo [ERROR] Failed to install %PKG_ID%.
  exit /b 1
)
echo [OK] Installed %PKG_ID%.
exit /b 0

:ensure_vs_build_tools
set "PKG_ID=Microsoft.VisualStudio.2022.BuildTools"
echo [INFO] Checking %PKG_ID% ...
set "WINGET_CHECK_LOG=%TEMP%\ccswitch_winget_check_%RANDOM%.log"
winget list --id "%PKG_ID%" -e --source winget --accept-source-agreements --disable-interactivity >"%WINGET_CHECK_LOG%" 2>&1
findstr /I /C:"%PKG_ID%" "%WINGET_CHECK_LOG%" >nul
if not errorlevel 1 (
  echo [OK] %PKG_ID% is already installed.
  del /q "%WINGET_CHECK_LOG%" >nul 2>&1
  exit /b 0
)
del /q "%WINGET_CHECK_LOG%" >nul 2>&1

echo [INFO] Installing %PKG_ID% with C++ workload and Windows SDK...
set "VS_OVERRIDE=--wait --passive --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.Windows11SDK.22621"
winget install --id "%PKG_ID%" -e --source winget --disable-interactivity --accept-source-agreements --accept-package-agreements --override "%VS_OVERRIDE%"
if errorlevel 1 (
  echo [ERROR] Failed to install %PKG_ID%.
  exit /b 1
)
echo [OK] Installed %PKG_ID%.
exit /b 0

:require_command
set "CMD_NAME=%~1"
where "%CMD_NAME%" >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Required command not found: %CMD_NAME%
  exit /b 1
)
echo [OK] Found command: %CMD_NAME%
exit /b 0

:run_step
set "STEP_CMD=%~1"
set "STEP_NAME=%~2"
echo [RUN ] %STEP_NAME%
cmd /c "%STEP_CMD%"
if errorlevel 1 (
  echo [ERROR] %STEP_NAME% failed.
  exit /b 1
)
echo [OK ] %STEP_NAME%
exit /b 0
