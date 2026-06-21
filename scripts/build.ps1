<#
.SYNOPSIS
    一键打包 cc-switch Tauri 桌面应用为可执行安装包。
.DESCRIPTION
    该脚本会检查构建环境依赖，安装 Node.js 依赖，然后执行 Tauri 构建，
    最终在 src-tauri/target/release/bundle 下生成安装包（MSI/EXE/DMG/AppImage）。
.PARAMETER SkipInstall
    跳过 pnpm install 步骤（已安装依赖时使用）。
.PARAMETER Debug
    构建 debug 版本（默认是 release）。
.PARAMETER Target
    指定构建目标（如 "x86_64-pc-windows-msvc"），留空则使用默认目标。
.PARAMETER Proxy
    指定网络代理地址（如 "http://localhost:7890"），留空则不配置代理。
.EXAMPLE
    .\scripts\build.ps1
    标准发布构建（推荐）。
.EXAMPLE
    .\scripts\build.ps1 -SkipInstall
    跳过依赖安装，直接构建。
.EXAMPLE
    .\scripts\build.ps1 -Debug
    构建 debug 版本。
#>

param(
    [switch]$SkipInstall,
    [switch]$Debug,
    [string]$Target = "",
    [string]$Proxy = "http://localhost:7890"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $PSCommandPath)

$profileLabel = if ($Debug) { "debug" } else { "release" }

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  CC Switch 一键打包脚本" -ForegroundColor Cyan
Write-Host "  版本: 3.16.2" -ForegroundColor Cyan
Write-Host "  配置: $profileLabel" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# ---------- 自动检测并配置网络代理 ----------
Write-Host "[1/5] 检测网络环境..." -ForegroundColor Yellow

function Test-GitHubAccess {
    try {
        $request = [System.Net.HttpWebRequest]::Create("https://github.com")
        $request.Timeout = 5000
        $request.Method = "HEAD"
        $response = $request.GetResponse()
        $response.Close()
        return $true
    } catch {
        return $false
    }
}

function Test-ProxyAvailable {
    param([string]$ProxyUrl)
    try {
        $request = [System.Net.HttpWebRequest]::Create("https://github.com")
        $request.Timeout = 5000
        $request.Method = "HEAD"
        $request.Proxy = New-Object System.Net.WebProxy($ProxyUrl)
        $response = $request.GetResponse()
        $response.Close()
        return $true
    } catch {
        return $false
    }
}

$hasDirectAccess = Test-GitHubAccess
if (-not $hasDirectAccess -and $Proxy -ne "") {
    Write-Host "  检测到 GitHub 无法直接访问，尝试使用代理: $Proxy" -ForegroundColor Yellow
    if (Test-ProxyAvailable -ProxyUrl $Proxy) {
        Write-Host "  [OK] 代理可用: $Proxy" -ForegroundColor Green
        # 设置环境变量代理（Cargo、rustc、Node.js 等均会读取）
        $env:HTTP_PROXY = $Proxy
        $env:HTTPS_PROXY = $Proxy
        $env:http_proxy = $Proxy
        $env:https_proxy = $Proxy

        # 配置 git 代理（用于 Cargo 拉取 git 依赖）
        $hasGitLocalProxy = git config --local http.proxy 2>$null
        if (-not $hasGitLocalProxy) {
            git config --local http.proxy $Proxy
            Write-Host "  [OK] 已配置 git local proxy" -ForegroundColor Green
        }
        Write-Host "  [提示] 代理仅在此脚本会话中生效，不影响系统设置" -ForegroundColor DarkYellow
    } else {
        Write-Host "  [警告] 指定的代理不可用: $Proxy，将尝试直接连接" -ForegroundColor Yellow
    }
} elseif ($hasDirectAccess) {
    Write-Host "  [OK] GitHub 可直接访问" -ForegroundColor Green
} else {
    Write-Host "  [警告] 无法访问 GitHub 且未指定代理，构建可能失败" -ForegroundColor Yellow
}

Write-Host ""

# ---------- 检查环境依赖 ----------
Write-Host "[2/5] 检查环境依赖..." -ForegroundColor Yellow

# 检查 Node.js
$nodeVersion = node --version 2>$null
if (-not $nodeVersion) {
    Write-Host "  [错误] 未找到 Node.js，请先安装: https://nodejs.org/" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Node.js $nodeVersion"

# 检查 pnpm
$pnpmVersion = pnpm --version 2>$null
if (-not $pnpmVersion) {
    Write-Host "  [错误] 未找到 pnpm，请执行: npm install -g pnpm" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] pnpm $pnpmVersion"

# 检查 Rust / cargo
$cargoVersion = cargo --version 2>$null
if (-not $cargoVersion) {
    Write-Host "  [错误] 未找到 Rust/Cargo，请先安装: https://rustup.rs/" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] $cargoVersion"

# 检查 Tauri CLI
$tauriVersion = pnpm tauri --version 2>$null
if (-not $tauriVersion) {
    Write-Host "  [警告] Tauri CLI 未安装，将使用 pnpm 自动安装" -ForegroundColor Yellow
} else {
    Write-Host "  [OK] Tauri CLI $tauriVersion"
}

Write-Host ""

# ---------- 安装依赖 ----------
if (-not $SkipInstall) {
    Write-Host "[3/5] 安装 Node.js 依赖..." -ForegroundColor Yellow
    Set-Location $ProjectRoot
    pnpm install
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  [错误] pnpm install 失败" -ForegroundColor Red
        exit 1
    }
    Write-Host "  [OK] 依赖安装完成"
} else {
    Write-Host "[3/5] 跳过依赖安装 (-SkipInstall)" -ForegroundColor DarkYellow
}
Write-Host ""

# ---------- 执行构建 ----------
Write-Host "[4/5] 开始 Tauri 构建 ($profileLabel)..." -ForegroundColor Yellow
Set-Location $ProjectRoot

$buildArgs = @("tauri", "build")
if ($Debug) {$buildArgs += "--debug" }
if ($Target -ne "") {$buildArgs += "--target"; $buildArgs +=$Target }

pnpm exec @buildArgs


if ($LASTEXITCODE -ne 0) {
    Write-Host "  [错误] Tauri 构建失败" -ForegroundColor Red
    exit 1
}
Write-Host ""

# ---------- 输出结果 ----------
Write-Host "[5/5] 构建完成!" -ForegroundColor Green

# 清理：移除临时配置的本地 git 代理
if (-not $hasDirectAccess -and $Proxy -ne "" -and (-not $hasGitLocalProxy)) {
    git config --local --unset http.proxy 2>$null
    Write-Host "  [OK] 已清理 git local proxy 配置" -ForegroundColor Green
}

# 确定产物目录
if ($Debug) {
    $bundleDir = Join-Path $ProjectRoot "src-tauri\target\debug\bundle"
} else {
    $bundleDir = Join-Path $ProjectRoot "src-tauri\target\release\bundle"
}

if (Test-Path $bundleDir) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "  构建产物位于:" -ForegroundColor Cyan
    Write-Host "  $bundleDir" -ForegroundColor White
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""

    $installers = Get-ChildItem -Recurse -File $bundleDir | Where-Object {
        $_.Extension -match '\.(msi|exe|dmg|AppImage|deb|rpm|apk)$'
    }
    if ($installers) {
        Write-Host "生成的安装包:" -ForegroundColor Green
        foreach ($installer in $installers) {
            $sizeInMB = [math]::Round($installer.Length / 1MB, 2)
            Write-Host "  - $($installer.FullName)  ($sizeInMB MB)" -ForegroundColor White
        }
    }
} else {
    Write-Host "  [提示] 未能找到 bundle 目录，请检查构建日志。" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "提示: 构建产物默认使用 'all' 目标，会生成当前平台所有可用的安装包格式。" -ForegroundColor Gray
Write-Host "      如需修改打包目标，请编辑 tauri.conf.json 中的 bundle.targets 字段。" -ForegroundColor Gray
