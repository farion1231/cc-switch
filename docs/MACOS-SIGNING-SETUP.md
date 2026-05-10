# macOS 发布手册

CC-Doctor 的 release 流水线（`.github/workflows/release.yml`）支持两种发布形态：

- **默认：未签名发布** — 零配置，打 tag 直接出 `.app` / `.dmg` / `.msi` / `.AppImage`
- **可选：Apple 签名 + 公证** — 配齐 6 项 Secret 后自动启用，消除 Gatekeeper 首次启动提示

CI 自动探测：6 项 `APPLE_*` Secret 全部就绪 → 走签名/公证；任一缺失 → 走未签名。**两种路径产物文件名一致，区别仅在是否带签名。**

---

## 0. 哪条路径适合你？

| 场景 | 推荐 | 用户首次启动体验 |
|---|---|---|
| 课堂演示 / 内部分发 / 原型验证 | **未签名（默认）** | 右键 → 打开（一次） |
| 公开分发 / 商业产品 / 减少用户疑虑 | **签名 + 公证** | 直接双击打开 |

---

# 第一部分：未签名发布（默认，零配置）

## 1.1 触发首次发布

打 tag 即可，无需任何 Secret：

```bash
git tag v3.14.2
git push origin v3.14.2
```

打开 <https://github.com/diaojz/cc-doctor/actions> 看流水线，约 20-30 分钟全平台产物上线 GitHub Releases。

## 1.2 产出清单

成功后 <https://github.com/diaojz/cc-doctor/releases> 可下载：

| 平台 | 文件 | 用法 |
|---|---|---|
| macOS | `CC-Doctor-vX.Y.Z-macOS.dmg` | 推荐，挂载后拖入 Applications |
| macOS | `CC-Doctor-vX.Y.Z-macOS.zip` | 备选，解压即用 |
| Windows | `CC-Doctor-vX.Y.Z-Windows.msi` | 安装器 |
| Windows | `CC-Doctor-vX.Y.Z-Windows-Portable.zip` | 绿色版 |
| Linux x86_64 | `CC-Doctor-vX.Y.Z-Linux-x86_64.AppImage` / `.deb` / `.rpm` | 三选一 |
| Linux arm64 | `CC-Doctor-vX.Y.Z-Linux-arm64.AppImage` / `.deb` / `.rpm` | 三选一 |

## 1.3 给 macOS 学员/用户的使用引导

把这一段直接发给学员：

> 1. 下载 `CC-Doctor-vX.Y.Z-macOS.dmg`
> 2. 双击 `.dmg` 挂载，把 `CC Doctor.app` 拖入 Applications
> 3. **首次启动**：在 Finder 找到 `CC Doctor.app`，**按住 Control 键点击图标 → 选择「打开」→ 弹窗里再次点击「打开」**
> 4. 之后双击启动恢复正常，无需重复

⚠️ 不要直接双击启动——会出现「无法打开'CC Doctor'，因为它来自身份不明的开发者」。这不是 App 的问题，是 macOS Gatekeeper 对未签名软件的默认拦截。

如果不小心双击被拦了，也能这样绕过：**系统设置 → 隐私与安全性 → 滚到底部「仍要打开」**。

## 1.4 Windows / Linux

- Windows：双击 `.msi` 安装；或解压 Portable.zip 后运行 `cc-doctor.exe`
- Linux AppImage：`chmod +x *.AppImage && ./*.AppImage`
- Linux deb：`sudo dpkg -i *.deb`
- Linux rpm：`sudo rpm -i *.rpm`

均无需额外操作。

---

# 第二部分：可选 — Apple 签名 + 公证

只在你想消除 Gatekeeper 提示、让用户「下载即用」时再做这一段。需要：

- 一个 Apple 开发者账号（年费 $99）
- Developer ID Application 证书
- 大约 30-60 分钟一次性配置

## 2.1 6 个 GitHub Secret 总览

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application 证书（.p12 → base64） |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 .p12 时设置的密码 |
| `KEYCHAIN_PASSWORD` | CI 临时 keychain 口令（自定义） |
| `APPLE_ID` | 你的 Apple ID 邮箱 |
| `APPLE_PASSWORD` | App-Specific Password（**不是登录密码**） |
| `APPLE_TEAM_ID` | 10 位 Team ID |

任一缺失 CI 都会自动回退到未签名路径，不会失败。

## 2.2 申请并导出 Developer ID Application 证书

### 2.2.1 在开发者后台创建证书

1. 打开 <https://developer.apple.com/account/resources/certificates/list>
2. 点 **+** 新建
3. 类型选 **Developer ID Application**（⚠️ 不是 Mac App Distribution，那个是 App Store 用的）
4. 按提示生成 CSR（Keychain Access → 菜单栏 Certificate Assistant → Request a Certificate From a Certificate Authority），上传后下载 `.cer`
5. 双击 `.cer` 导入到 Keychain

### 2.2.2 从 Keychain 导出 .p12

1. 打开 **Keychain Access**
2. 左侧切到 **login** keychain，右侧筛选 **My Certificates**
3. 找到 `Developer ID Application: 你的名字 (TEAM_ID)`，**展开三角箭头**，确认下面有私钥
4. **右键证书 → Export**，文件类型选 **Personal Information Exchange (.p12)**，保存（如 `~/Desktop/cc-doctor-cert.p12`）
5. 设置一个导出密码（这就是 `APPLE_CERTIFICATE_PASSWORD`），**记下来**

### 2.2.3 转 base64

```bash
base64 -i ~/Desktop/cc-doctor-cert.p12 | pbcopy
```

剪贴板里就是 `APPLE_CERTIFICATE` 的值。

## 2.3 生成 App-Specific Password

`xcrun notarytool` 公证不接受 Apple ID 主密码。

1. 登录 <https://appleid.apple.com>
2. 找到 **App-Specific Passwords / 应用专用密码**
3. **+ Generate**，标签写 `cc-doctor-notary`
4. 出来的 `abcd-efgh-ijkl-mnop` 格式密码 **只显示一次**，立即复制
5. 这就是 `APPLE_PASSWORD`

## 2.4 找到 Team ID

<https://developer.apple.com/account> 右上角 Membership 区域，10 位 Team ID（如 `9ABCDEFGHI`）。这就是 `APPLE_TEAM_ID`。

## 2.5 自定义 KEYCHAIN_PASSWORD

随便生成一个强口令：

```bash
openssl rand -base64 24 | pbcopy
```

剪贴板内容直接作为 `KEYCHAIN_PASSWORD`。

## 2.6 在 GitHub 仓库填 Secret

打开 <https://github.com/diaojz/cc-doctor/settings/secrets/actions>，逐项 **New repository secret**：

| Name | Value 来源 |
|---|---|
| `APPLE_CERTIFICATE` | §2.2.3 剪贴板 |
| `APPLE_CERTIFICATE_PASSWORD` | §2.2.2 设置的密码 |
| `KEYCHAIN_PASSWORD` | §2.5 剪贴板 |
| `APPLE_ID` | 你的 Apple ID 邮箱 |
| `APPLE_PASSWORD` | §2.3 生成的应用专用密码 |
| `APPLE_TEAM_ID` | §2.4 的 10 位字符 |

## 2.7 触发签名发布

仍然是打 tag。CI 的 `Check Apple signing prerequisites` step 会探测这 6 项 Secret 是否齐全：

- 全有 → 自动走签名 + 公证，产出可直接双击打开
- 任一缺失 → 自动回退未签名路径，产出仍可分发但首启需「右键 → 打开」

```bash
git tag v3.14.3
git push origin v3.14.3
```

## 2.8 验证产出（仅签名版本）

下载新 dmg，挂载后用户应该 **直接双击就能打开**，没有任何弹窗。本机也能验证：

```bash
spctl -a -t exec -vv /Applications/CC\ Doctor.app
# 期望：accepted, source=Notarized Developer ID

codesign -dv --verbose=4 /Applications/CC\ Doctor.app | grep Authority
# 期望第一行：Authority=Developer ID Application: 你的名字 (TEAM_ID)

xcrun stapler validate /Applications/CC\ Doctor.app
# 期望：The validate action worked!
```

CI 内部已经自动跑了同款验证（见 release.yml 的 `Verify macOS bundle` step），所以本地这步可选。

## 2.9 常见错误

### 2.9.1 `❌ No 'Developer ID Application' identity found`

证书类型选错了。回到 §2.2.1 重新申请，类型必须是 **Developer ID Application**。

### 2.9.2 公证失败 `Status: Invalid`

通常原因：

- App-Specific Password 不对/过期 → 重新生成
- Apple ID 没接受最新开发者协议 → 登录 <https://developer.apple.com/account> 接受
- Team ID 写错 → 在开发者后台 Membership 页面再确认
- 二进制中夹带了未签名的 native 库 → 看 `notarytool log <submission-id>` 的具体报错

### 2.9.3 配了 Secret 但 CI 仍走未签名路径

打开 `Check Apple signing prerequisites` step 的日志，看具体哪一项 Secret 为空。注意 Secret 名字大小写必须完全匹配。

### 2.9.4 用户首次启动仍被拦

确认 release.yml 跑的是签名路径（在 `Check Apple signing prerequisites` 看到 `available=true`），且 `Verify macOS bundle` step 里走的是签名分支（输出 `✅ codesign verification passed` 等）。如果都对，让用户**直接从 GitHub Releases 链接下载**，避免某些中转工具去除扩展属性。

---

# 附录：未来恢复 Tauri Updater 自动更新

当前 `src-tauri/tauri.conf.json` 的 `bundle.createUpdaterArtifacts` 设为 `false`，App 不带自动更新能力。如果未来想恢复：

1. 把 `createUpdaterArtifacts` 改回 `true`
2. 把本地 `~/.tauri/cc-doctor.key`（fork 时已生成）的 base64 配为 `TAURI_SIGNING_PRIVATE_KEY` Secret：
   ```bash
   base64 -i ~/.tauri/cc-doctor.key | tr -d '\n' | pbcopy
   ```
3. 在 release.yml 中重新加入 updater 相关 step（参考 git 历史，commit `1c3c7d37` 之前的版本）

短期（教学场景、未公开发布）建议保持当前禁用状态，避免无谓的密钥管理负担。
