@echo off
setlocal
cd /d "%~dp0.."
if not exist ".\node_modules\.bin\tauri.cmd" (
  echo Local Tauri CLI missing. Install project dependencies first.
  exit /b 1
)
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 exit /b 1
set "CFLAGS=/nologo /MD /Z7"
set "AWS_LC_SYS_CFLAGS=/nologo /MD /Z7 /W3 /we4013"
set "CRATE_CC_NO_DEFAULTS=1"
set "npm_config_manage_package_manager_versions=false"
if not defined CARGO_BUILD_JOBS set "CARGO_BUILD_JOBS=1"
call ".\node_modules\.bin\tauri.cmd" build --no-bundle %*
set "BUILD_EXIT=%ERRORLEVEL%"
endlocal & exit /b %BUILD_EXIT%
