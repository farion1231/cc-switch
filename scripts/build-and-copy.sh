#!/bin/bash

# 构建并复制最新的应用程序文件到 docs/downloads 目录
# 用法: ./scripts/build-and-copy.sh

set -e

echo "🚀 开始构建 CC Switch..."

# 获取版本号
VERSION=$(grep '"version"' package.json | head -1 | sed 's/.*"version": "\(.*\)".*/\1/')
echo "📦 当前版本: $VERSION"

# 构建应用
echo "🔨 执行构建..."
npm run tauri build

# 检测平台
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    echo "🍎 检测到 macOS 平台"
    
    # 获取架构
    ARCH=$(uname -m)
    if [[ "$ARCH" == "arm64" ]]; then
        ARCH_NAME="aarch64"
    else
        ARCH_NAME="x64"
    fi
    
    DMG_PATH="src-tauri/target/release/bundle/dmg/CC Switch_${VERSION}_${ARCH_NAME}.dmg"
    
    if [ -f "$DMG_PATH" ]; then
        echo "📋 复制 DMG 文件到 docs/downloads..."
        cp "$DMG_PATH" "docs/downloads/"
        echo "✅ 已复制: CC Switch_${VERSION}_${ARCH_NAME}.dmg"
    else
        echo "❌ 未找到 DMG 文件: $DMG_PATH"
        exit 1
    fi
    
elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    # Windows
    echo "🪟 检测到 Windows 平台"
    
    MSI_PATH="src-tauri/target/release/bundle/msi/CC Switch_${VERSION}_x64_en-US.msi"
    
    if [ -f "$MSI_PATH" ]; then
        echo "📋 复制 MSI 文件到 docs/downloads..."
        cp "$MSI_PATH" "docs/downloads/"
        echo "✅ 已复制: CC Switch_${VERSION}_x64_en-US.msi"
    else
        echo "❌ 未找到 MSI 文件: $MSI_PATH"
        exit 1
    fi
    
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    # Linux
    echo "🐧 检测到 Linux 平台"
    
    APPIMAGE_PATH="src-tauri/target/release/bundle/appimage/cc-switch_${VERSION}_amd64.AppImage"
    
    if [ -f "$APPIMAGE_PATH" ]; then
        echo "📋 复制 AppImage 文件到 docs/downloads..."
        cp "$APPIMAGE_PATH" "docs/downloads/"
        echo "✅ 已复制: cc-switch_${VERSION}_amd64.AppImage"
    else
        echo "❌ 未找到 AppImage 文件: $APPIMAGE_PATH"
        exit 1
    fi
fi

echo ""
echo "🎉 构建完成！"
echo "📁 构建文件已复制到: docs/downloads/"
echo ""
echo "📝 下一步:"
echo "   1. 检查 docs/downloads/ 目录中的文件"
echo "   2. 测试安装包是否正常工作"
echo "   3. 创建 GitHub Release 并上传文件"
