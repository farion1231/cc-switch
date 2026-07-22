# Fork：同步官方 + 自己的应用内更新

本 fork（`xjwm5685-ui/cc-switch-pro`）与官方（`farion1231/cc-switch`）分离：

- **官方更新**：只合进你的分支，不推回官方
- **应用内更新**：只装你自己的 Release，不会被官方包盖掉

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

产品线在 `feat/pi-support` 时：手动跑 workflow，把 `target_branch` 填成 `feat/pi-support`。

受保护分支推送失败时，在 fork 仓库 Secrets 增加：

- `SYNC_UPSTREAM_TOKEN`：PAT，权限 `contents:write` + `pull_requests:write`

---

## 3) 应用内更新 → 只指向本 fork

`src-tauri/tauri.conf.json`：

```json
"endpoints": [
  "https://github.com/xjwm5685-ui/cc-switch-pro/releases/latest/download/latest.json"
]
```

官方公钥**验不过**你自己打的包（你没有官方私钥）。需使用本机已生成的密钥对：

| 文件 | 作用 |
|------|------|
| `%USERPROFILE%\.tauri\cc-switch-pro.key` | **私钥**，绝不能进 git |
| `%USERPROFILE%\.tauri\cc-switch-pro.key.pub` | 公钥 → 已写入 `plugins.updater.pubkey` |

### 你还差这一步（只做一次）

在 GitHub **本 fork** → Settings → Secrets and variables → Actions 添加：

1. `TAURI_SIGNING_PRIVATE_KEY` = `cc-switch-pro.key` 的**完整文件内容**（含 `untrusted comment:` 那两行）
2. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 空（生成时用了空密码；若你后来改过密码再填）

然后打 tag 触发 `Release` 工作流，会发布带签名的安装包 + `latest.json`。之后应用内「检查更新」只会升你自己的版本。

重新生成密钥（会作废旧签名，慎用）：

```powershell
pnpm tauri:signer:generate -- -Force
```

---

## 推荐日常流

```text
pnpm sync:upstream:push     # 或等 GitHub Action 开 PR
# 解决冲突 / 审 PR
# 开发你的功能 → push fork
# 发版：打 tag → Release CI → 用户从你的渠道更新
```
