#!/bin/bash

# 验证 ClaudeAuth 认证修复的脚本
# 用于测试 stream_check.rs 中的修复是否正确处理不同的认证策略

set -e

echo "======================================"
echo "验证 ClaudeAuth 认证修复"
echo "======================================"
echo ""

cd "$(dirname "$0")/src-tauri"

echo "1. 运行 Rust 测试..."
cargo test stream_check::tests --quiet
echo "✅ 测试通过"
echo ""

echo "2. 运行 Clippy 检查..."
cargo clippy --quiet 2>&1 | grep -i "error\|warning.*stream_check" || echo "✅ 无相关问题"
echo ""

echo "3. 编译检查..."
cargo check --quiet 2>&1 | grep -i "error" || echo "✅ 编译成功"
echo ""

echo "4. 检查代码修改..."
echo "修改的文件:"
git diff --name-only src/services/stream_check.rs
echo ""

echo "======================================"
echo "验证完成!"
echo "======================================"
echo ""
echo "修改摘要:"
echo "- 添加了 AuthStrategy 导入"
echo "- 修改了 check_claude_stream 函数,根据 auth.strategy 决定是否添加 x-api-key"
echo "- 添加了单元测试验证 AuthStrategy 枚举"
echo ""
echo "预期行为:"
echo "- AuthStrategy::Anthropic: 添加 Authorization + x-api-key"
echo "- AuthStrategy::ClaudeAuth: 仅添加 Authorization (不添加 x-api-key)"
echo "- AuthStrategy::Bearer: 仅添加 Authorization"
echo ""
