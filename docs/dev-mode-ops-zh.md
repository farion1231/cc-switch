# 开发模式运维说明

本文档记录本项目在本地开发模式下的常用启动、运行与停止命令。

## 适用范围

- 仓库路径：`/Users/xionghaoqiang/cc_sw`
- 前端：Vite
- 桌面端：Tauri
- 包管理器：`pnpm`

## 环境前提

启动前请确认本机已安装以下依赖：

- `pnpm`
- Rust 1.85+
- Tauri CLI 2.8+

首次拉起项目时，先在仓库根目录安装依赖：

```bash
pnpm install
```

## 启动命令

推荐使用项目内置脚本启动：

```bash
./scripts/dev.sh
```

脚本行为如下：

- 自动切换到仓库根目录
- 检查 `pnpm` 是否可用
- 若缺少 `node_modules`，自动执行 `pnpm install`
- 最终执行 `pnpm dev`

如需直接使用原始命令，也可以运行：

```bash
pnpm dev
```

对应脚本定义：

```bash
pnpm tauri dev
```

## 运行说明

启动成功后，通常会看到以下行为：

- Vite 开发服务器启动
- 本地前端地址可访问：`http://localhost:14211/`
- Tauri 桌面窗口自动拉起
- `src/` 和 `src-tauri/` 的修改会触发热更新或重新编译

## 停止方式

如果开发命令在当前终端前台运行，直接按：

```bash
Ctrl+C
```

这会停止当前的 Vite 与 Tauri 开发进程。

## 常用辅助命令

类型检查：

```bash
pnpm typecheck
```

格式化代码：

```bash
pnpm format
```

检查格式：

```bash
pnpm format:check
```

运行前端单元测试：

```bash
pnpm test:unit
```

监听模式运行测试：

```bash
pnpm test:unit:watch
```

构建正式版本：

```bash
pnpm build
```

构建调试版本：

```bash
pnpm tauri build --debug
```

## 故障排查

若启动失败，可按以下顺序检查：

1. 当前目录是否为仓库根目录。
2. `pnpm` 是否已安装并在 `PATH` 中。
3. `node_modules` 是否完整；必要时重新执行 `pnpm install`。
4. Rust 与 Tauri CLI 版本是否满足要求。
5. 端口 `14211` 是否已被其他进程占用。
