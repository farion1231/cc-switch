param(
    [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"

# 定位工具目录，允许从任意路径调用本脚本。
function Get-ToolRoot {
    $scriptDir = Split-Path -Parent $PSCommandPath
    return (Resolve-Path $scriptDir).Path
}

# 创建本地虚拟环境并安装 PyInstaller，避免污染用户全局 Python 环境。
function Ensure-BuildPython {
    param([string]$ToolRoot)

    $venvDir = Join-Path $ToolRoot ".venv"
    $pythonExe = Join-Path $venvDir "Scripts\python.exe"
    if (-not (Test-Path -LiteralPath $pythonExe)) {
        python -m venv $venvDir
    }

    $null = & $pythonExe -m pip install --upgrade pip
    $null = & $pythonExe -m pip install pyinstaller
    return $pythonExe
}

# 生成 Python zip、Windows exe 和校验文件，供 GitHub Release 直接上传。
function Write-ReleaseAssets {
    param(
        [string]$ToolRoot,
        [string]$PythonExe,
        [string]$Version
    )

    $distDir = Join-Path $ToolRoot "dist"
    $buildDir = Join-Path $ToolRoot "build"
    $releaseDir = Join-Path $ToolRoot "release"
    Remove-Item -LiteralPath $distDir, $buildDir, $releaseDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

    $exeBaseName = "CCSwitchCodexHistoryTool_$Version`_windows_x64"
    & $PythonExe -m PyInstaller `
        --onefile `
        --clean `
        --name $exeBaseName `
        --distpath $releaseDir `
        --workpath $buildDir `
        --specpath $buildDir `
        (Join-Path $ToolRoot "codex_history_tool.py")

    $pythonStage = Join-Path $distDir "python"
    New-Item -ItemType Directory -Force -Path $pythonStage | Out-Null
    Copy-Item -LiteralPath (Join-Path $ToolRoot "codex_history_tool.py") -Destination $pythonStage -Force
    Copy-Item -LiteralPath (Join-Path $ToolRoot "README.md") -Destination $pythonStage -Force
    Compress-Archive -Path (Join-Path $pythonStage "*") -DestinationPath (Join-Path $releaseDir "CCSwitchCodexHistoryTool_$Version`_python.zip") -Force

    $checksumPath = Join-Path $releaseDir "SHA256SUMS-v$Version.txt"
    Get-ChildItem -LiteralPath $releaseDir -File |
        Where-Object { $_.Name -ne "SHA256SUMS-v$Version.txt" } |
        Sort-Object Name |
        ForEach-Object {
            $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash
            "$hash  $($_.Name)"
        } |
        Set-Content -Encoding ASCII -LiteralPath $checksumPath
}

$toolRoot = Get-ToolRoot
$pythonExe = Ensure-BuildPython -ToolRoot $toolRoot
Write-ReleaseAssets -ToolRoot $toolRoot -PythonExe $pythonExe -Version $Version
Write-Host "Release assets written to: $(Join-Path $toolRoot 'release')"
