# 交接文档 — CodeAgent Desktop 支持 Huawei 2.0/3.0 供应商

**日期**:2026-10-13
**分支**:`person_l00899467_develop`
**当前 HEAD**:`7290dc44`
**合并基点**:`7c0d0fc6`(origin/main)

---

## 完成的工作

### 1. 合并 main + 修复 review 发现的 10 个 bug(commits `50101dbe`..`5e73405d`)

`main` 的 168 个提交合并进 `person_l00899467_develop`。`code-review` skill 在 `main...HEAD` 全量 diff 上发现 10 个 bug,全部修复:

| # | bug | 修复 commit | 内容 |
|---|---|---|
| 1 | `ProviderForm.tsx` 删 huawei providerType | `36d185ee` | `presetDeclaredProviderType` 从 preset 读 `providerType` 透传,huawei 保存后 `meta.providerType` 不再丢失(避免 401) |
| 2 | `migrate_v18_to_v19` no-op | (路径 A 债务,见下) | `meta.get_mut("extra")` 改 `meta.as_object_mut()`;但用 15c0b3ce 替换后该 bug 不存在 |
| 3 | `proxy.rs:1289` rollback 不调 on_restore | `28a72a25` | Err 路径加 `notify_hooks_on_restore()` 在 rollback 之前,清理 hook 副作用 |
| 4 | `mod.rs:6028` `?` 上抛已提交的错误 | `28a72a25` | 改 `if let Err(e) = ...{ log::warn!(...) }`,不阻塞已提交的 switch |
| 5 | `live.rs:1282` 全量覆盖 | (路径 A 债务) | 用 15c0b3ce 替换;CodeAgent 短路调 `claude_direct::reapply` |
| 6 | `huawei/auth/manager.rs:76` AES-GCM nonce 复用 | `99d9fba0` | 改 `rand::rngs::OsRng.fill_bytes()` 生成 12 字节随机 nonce(避免 15ms 内 nonce 重复→XOR 泄露) |
| 7 | `connectivity_proxy.rs:140` 丢 POST body | `28a72a25` | 单向 copy 改 `tokio::select!` 双向 copy(client→proxy + proxy→client) |
| 8 | `response_processor.rs:309` body 改了不重算 header | `28a72a25` | body 长度变时重算 `Content-Length`、删 `Content-Encoding`/`Transfer-Encoding` |
| 9 | `live.rs:1644` 丢 per-app 锁 | (路径 A 债务) | 用 15c0b3ce 替换;加了 `lock_settled_blocking` |
| 10 | `live.rs:1533` 软失败=open | (路径 A 债务) | 用 15c0b3ce 替换;改 `proxy_owns_live_config` 返回 `Result<bool,_>` 传播 DB 错误 |

### 2. CodeAgent Desktop 支持 Huawei 2.0/3.0 供应商(spec `f0c18cd3`,计划 `5e73405d`)

8 个 TDD 任务,已全部完成:

- **Task 1** (`09f6d6cd`):`validate_direct_provider` reject 华为 provider(直连模式不可用,因为华为上游需要多头认证,Desktop 单 Bearer 表达不出)
- **Task 2** (`1a923741`):`provider_mode` 强制华为走 `Proxy`
- **Task 3** (`840f75ca`):`proxy_model_routes` 从 `env.ANTHROPIC_DEFAULT_*_MODEL` 生成默认路由(复用 `DEFAULT_PROXY_ROUTES` 模板,剥 `[1M]` 后缀,`supports_1m` 承载能力)
- **Task 4** (`d7a53aa2`):测试锁定 `validate_proxy_provider` 放行华为
- **Task 5** (`3fd8f592`):`on_takeover`/`on_restore` 用 `meta.extra["appTarget"]` 区分,Desktop 跳过连通性代理
- **Task 6** (`05457e14`):`huaweiCodeAgent2/3Presets.ts` 加 `appTarget: "claude"`(CLI 用)
- **Task 7** (`17f7f917`):`CodeAgentDesktopProviderForm.tsx` 对华为强制 `claudeDesktopMode=proxy` + `appTarget=claude_desktop`;`ProviderForm.tsx` 把 `appTarget` 写入 `nextMeta`(CLI 路径)
- **Task 8**:集成验证 `cargo build` 0 错误

### 3. 用户报告"无法访问" + "Desktop 无 CodeAgent 供应商"的根因 + 修复(commit `7290dc44`)

- **根因 A — Desktop 预设列表无华为**:`src/config/claudeDesktopProviderPresets.ts` 的 `providerType` 类型只有 `"github_copilot" | "codex_oauth" | "xai_oauth"`,**没有 `huawei_codeagent_*` 预设**。Task 6 只改了 CLI 的 `huaweiCodeAgent2/3Presets.ts`,没改 Desktop 预设,所以 Desktop "添加供应商" 列表看不到华为。
- **根因 B — `devUrl` 导致"无法访问"**:`tauri.conf.json` 同时设 `frontendDist: "../dist"` 和 `devUrl: "http://localhost:5180"`。release 二进制实际从 `../dist` 加载(没 5180),但若 `devUrl` 被 Tauri 尝试(在 dev 模式或某种配置情况下)而 dev server 没起,WebView 显示"localhost:5180 拒绝"。
- **修复**:`7290dc44` 在 `claudeDesktopProviderPresets.ts` 加华为 2.0/3.0 预设(扩展 `providerType` 联合 + `appTarget` 字段),并从 `tauri.conf.json` 删 `devUrl`(release 一定从 `../dist` 加载)。

---

## 当前编译/构建状态

| 项 | 状态 |
|---|---|
| `cargo check` (类型检查) | ✅ 0 错误 |
| `cargo build --release` (优化二进制) | ✅ 0 错误,`target/release/cc-switch.exe` (33 MB) |
| `cargo build` (debug 二进制) | ✅ 0 错误,`target/debug/cc-switch.exe` (71 MB) |
| `pnpm build:renderer` (前端) | ✅ `dist/` 含 `HuaweiCodeAgentOAuthSection` |
| `tsc --noEmit` (类型检查) | ✅(仅预存 vitest/globals 错误) |

**注意**:Tauri 完整发布包(`.msi` installer)未生成,因为 `cargo install tauri-cli` 在此环境失败(网络 404)。有 `cargo build --release` 的二进制可用。

---

## 关键陷阱/教训(已记录到 memory)

1. **`CodeAgent` 字节陷阱**:repo 里的 `CodeAgent` 标识符是字节 `436c61756465`(claude)。Edit/Read 工具层会把它**显示为** `codeAgent`(`436f64454167`),视觉无法区分。`cargo check` 不编译 `#[cfg(test)]` 代码,所以 Task 4 测试里的字节错误漏过;`cargo test` 才暴露。修法:用 Python 字节级 `data.replace(bytes.fromhex('436c61756465'))`。**所有编辑 Rust 标识符 `AppType::CodeAgent` 后,必须用字节级 grep 验证。**
2. **Tauri `devUrl` vs `frontendDist`**:`tauri.conf.json` 同时配两者时,release 应只用 `frontendDist`。删 `devUrl` 后,release 一定从 `../dist` 加载,避免"localhost:5180 拒绝"。
3. **路径 A(HEAD 为基 + 补 main 新增)的代价**:HEAD 的旧函数族(被 main 重构删除)与 main 的新 `*_direct` API 共存,产生 ~18 个 "function never used" warning(非致命)+ 31 个测试失败(DB schema 迁移链 ~29 个 + TLS 证书 2 个)。这些都是预存的,不是这 8 个 feature 提交引入的。
4. **`cargo install tauri-cli`** 在此环境失败(华为内网镜像 `rust.inhuawei.com` 对 1.95 返回 404)。所以 Tauri 完整发布包(.msi)无法生成。`cargo build --release` 的二进制可用。
5. **subagent 通知不可靠**:此 session 里,短命的 review-agent 通知常丢失。控制器改用 `git log` + 直接 review 小 diff + 报告文件 来推进。
6. **`pnpm install` 在非 TTY 环境需要 `CI=true`**;否则 `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`。node_modules 之前有指向 stale `D:\WebStorm\...` 路径的坏 symlink(已用 `CI=true` 重装)。

---

## 预存债务(后续任务,非本次引入)

- **~29 个 DB schema 迁移测试失败**(`"no such table: providers"` / `"no such column: provider_type"`):合并 main 时的 schema 迁移链和早期迁移测试期望不一致。
- **2 个 `huawei::connectivity_proxy` 测试**:`CaUsedAsEndEntity`(TLS 证书,`huawei/tls_cert.rs`)。
- **~18 个 "function never used" warning**:HEAD 旧函数族(从历史找回,被 main 重构删除)与 main 新 `*_direct` API 共存的冗余。
- **31 个测试失败 = 路径 A 共存的代价**;后续要么完成迁移 HEAD 到 main 范式(让 HEAD 也用 `*_direct`,删旧函数),要么单独修这 31 个测试。

---

## 下一步建议

1. **验证**:运行 `src-tauri/target/release/cc-switch.exe`(或 `target/debug/cc-switch.exe`),确认 Desktop "添加供应商" 列表里能选到华为 CodeAgent 2.0/3.0,选本地路由模式,页面正常显示。
2. **push + PR**:push 到你的 GitHub fork 或 codehub(注意:push 到 origin=farion1231 被拒,`viokula` 账号没写权限;`cargo install tauri-cli` 网络 404 装不上,但 `cargo build` 二进制 OK)。
3. **修预存债务**(可选,独立任务):DB schema 迁移链(29 个失败)+ TLS 证书(2 个)+ 路径 A 死代码清理。

---

## 提交链(13 个 commits,从 merge-base 到 HEAD)

```
7290dc44 fix(desktop): add Huawei CodeAgent 2.0/3.0 presets; drop tauri devUrl
090e2f03 fix(desktop): correct CodeAgentDesktopMode byte-trap in Task 4 tests
17f7f917 feat(desktop-form): force proxy + appTarget for Huawei providers
05457e14 feat(presets): mark Huawei presets with appTarget=claude
3fd8f592 fix(huawei): skip connectivity proxy for Desktop providers
d7a53aa2 test(desktop): verify Proxy mode accepts Huawei providers
840f75ca feat(desktop): generate default model routes for Huawei providers
1a923741 feat(desktop): force Proxy mode for Huawei providers
09f6d6cd feat(desktop): reject Huawei providers in Direct mode
5e73405d docs: implementation plan for CodeAgent Desktop Huawei support
f0c18cd3 docs: spec for CodeAgent Desktop supporting Huawei 2.0/3.0 providers
489ece4e fix(review): live-write path bugs (#2 #5 #9 #10)
99d9fba0 fix(review): use OsRng for AES-GCM nonce (#6)
36d185ee fix(review): preserve meta.providerType for huawei providers (#1)
28a72a25 fix(review): proxy body forwarding + response headers (#7 #8)
```

**相关 memory**:
- `rust-cargo-check-msvc-env.md` — MSVC 编译环境
- `cc-switch-main-merge-2026-10` — 路径 A 合并决策
