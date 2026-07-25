# Fork：同步官方（不含劫持官方更新通道）

本仓库可同时服务：

1. **上游贡献**（PR → `farion1231/cc-switch`）：必须保留官方 updater
2. **个人 fork 发版**（`xjwm5685-ui/cc-switch-pro`）：仅在 **fork 默认分支 / release 分支** 上改 updater

## 硬性规则（合并到官方前必查）

- `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints` **必须** 指向：
  `https://github.com/farion1231/cc-switch/releases/latest/download/latest.json`
- `plugins.updater.pubkey` **必须** 保持官方公钥
- **禁止**把 fork 的 endpoint / pubkey 放进针对上游 `main` 的 PR

fork 自有更新通道只在 fork 仓库本地改，且不要回提上游。

本地 remote 约定：

| remote | 仓库 |
|--------|------|
| `origin` | 官方 `farion1231/cc-switch`（只 fetch / merge） |
| `fork` | 你的 `xjwm5685-ui/cc-switch-pro`（push 目标） |

---

## 1) 本地一键同步官方

```powershell
# 把 origin/main 合并进当前分支
pnpm sync:upstream

# 工作区有未提交改动时自动 stash，合并后 push 到 fork
pnpm sync:upstream:push
```

脚本：`scripts/sync-upstream.ps1`  
**不会**往官方 `origin` push；`-Push` 只推 `fork`。

有冲突时按脚本提示解决后：

```powershell
git add -A
git commit
git push fork HEAD
```

---

## 2) GitHub Action 自动同步

工作流：`.github/workflows/sync-upstream.yml`

- 每天定时 + 可手动 Run workflow
- 默认：往本 fork 的 `main` **开 PR**（审完再合，最安全）
- 可选：`create_pr_only=false` → 无冲突时直接合入目标分支

受保护分支推送失败时，在 fork 仓库 Secrets 增加：

- `SYNC_UPSTREAM_TOKEN`：PAT，权限 `contents:write` + `pull_requests:write`

---

## 3) Fork 专用应用内更新（仅 fork 仓库，勿提上游）

仅在 **fork 自己的 release 线** 上覆盖 `tauri.conf.json`：

```json
"endpoints": [
  "https://github.com/xjwm5685-ui/cc-switch-pro/releases/latest/download/latest.json"
]
```

并换成你自己的 `pubkey`。官方公钥验不过你签名的包。

| 文件 | 作用 |
|------|------|
| `%USERPROFILE%\.tauri\cc-switch-pro.key` | **私钥**，绝不能进 git |
| `%USERPROFILE%\.tauri\cc-switch-pro.key.pub` | 公钥 → 只写进 **fork** 的 `plugins.updater.pubkey` |

在 GitHub **本 fork** → Settings → Secrets → Actions：

1. `TAURI_SIGNING_PRIVATE_KEY` = `cc-switch-pro.key` 完整内容
2. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 空（若生成时用了空密码）

```powershell
pnpm tauri:signer:generate -- -Force
```

---

## 推荐日常流

```text
pnpm sync:upstream:push     # 或等 GitHub Action 开 PR
# 解决冲突 / 审 PR
# 功能 PR → 推 origin（官方 updater 不变）
# fork 发版：在 fork 分支改 updater → 打 tag → Release CI
```
