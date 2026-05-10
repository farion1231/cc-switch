# macOS 签名与公证发布手册

CC-Doctor 的 release 流水线（`.github/workflows/release.yml`）已经把 macOS 签名 + 公证 + DMG 美化 + universal binary 全部串好了。**你需要做的只是把对应的密钥/证书塞进 GitHub Actions Secrets，然后打 tag 触发即可。**

> 目标：用户下载 `CC-Doctor-vX.Y.Z-macOS.dmg`（或 `.zip`），双击拖入 Applications，**不会被 Gatekeeper 拦截**。

---

## 0. 总览

CI 跑通需要 8 个 Secret，按用途分三组：

| 组别 | Secret | 用途 |
|---|---|---|
| **macOS 签名** | `APPLE_CERTIFICATE` | Developer ID Application 证书（base64 后的 .p12） |
| | `APPLE_CERTIFICATE_PASSWORD` | 导出 .p12 时设置的密码 |
| | `KEYCHAIN_PASSWORD` | CI 临时 keychain 的口令（自定义任意强口令） |
| **macOS 公证** | `APPLE_ID` | 你的 Apple ID 邮箱 |
| | `APPLE_PASSWORD` | App-Specific Password（**不是登录密码**） |
| | `APPLE_TEAM_ID` | 10 位 Team ID |
| **Tauri Updater** | `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater 私钥（已生成，见 §4） |
| | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥口令（本项目无口令，可留空或不创建） |

---

## 1. 申请并导出 Developer ID Application 证书

### 1.1 在开发者后台创建证书

1. 打开 <https://developer.apple.com/account/resources/certificates/list>
2. 点 **+** 新建
3. 类型选 **Developer ID Application**（⚠️ 不是 Mac App Distribution，那个是给 App Store 走的）
4. 按提示生成 CSR（Keychain Access → 菜单栏 Certificate Assistant → Request a Certificate From a Certificate Authority），上传后下载 `.cer`
5. 双击 `.cer` 导入到 Keychain

### 1.2 从 Keychain 导出 .p12

1. 打开 **Keychain Access**
2. 左侧切到 **login** keychain，右侧筛选 **My Certificates**
3. 找到 `Developer ID Application: 你的名字 (TEAM_ID)`，**展开三角箭头**，确认下面有私钥
4. **右键证书 → Export**，文件类型选 **Personal Information Exchange (.p12)**，保存到任意位置（如 `~/Desktop/cc-doctor-cert.p12`）
5. 设置一个导出密码（这个就是后面的 `APPLE_CERTIFICATE_PASSWORD`），**记下来**

### 1.3 转 base64

```bash
base64 -i ~/Desktop/cc-doctor-cert.p12 | pbcopy
```

剪贴板里就是 `APPLE_CERTIFICATE` 的值。

---

## 2. 生成 App-Specific Password

`xcrun notarytool` 公证时不能用你的 Apple ID 主密码，必须用专用密码。

1. 登录 <https://appleid.apple.com>
2. 找到 **App-Specific Passwords / 应用专用密码**
3. 点 **+ Generate**，标签写 `cc-doctor-notary`
4. 出来的格式形如 `abcd-efgh-ijkl-mnop`，**只显示一次**，立即复制保存
5. 这就是 `APPLE_PASSWORD`

---

## 3. 找到 Team ID

打开 <https://developer.apple.com/account>，右上角 Membership 区域，**Team ID** 是一串 10 位字符（如 `9ABCDEFGHI`）。这就是 `APPLE_TEAM_ID`。

---

## 4. Tauri Updater 私钥

> **私钥已经在本机生成完毕**，路径：`~/.tauri/cc-doctor.key`。
> 对应公钥已经写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。

### 4.1 取出私钥的 base64 形式

```bash
base64 -i ~/.tauri/cc-doctor.key | tr -d '\n' | pbcopy
```

剪贴板里就是 `TAURI_SIGNING_PRIVATE_KEY` 的值。CI 里 `Prepare Tauri signing key` step 会自动识别这种 base64 包裹格式并还原。

### 4.2 关于 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

本项目生成私钥时使用了 `--password ""`（无口令），所以这个 Secret **可以不创建**。如果你以后重新生成带口令的私钥，再补上即可。

### 4.3 ⚠️ 私钥保管

`~/.tauri/cc-doctor.key` 是**唯一**能为 updater 包签名的私钥。**丢失等于以后所有版本都无法走自动更新**——用户必须手动重新下载安装。建议：

- 把 `~/.tauri/cc-doctor.key` 备份到 1Password / iCloud Keychain / 加密 U 盘
- 不要提交到任何 Git 仓库
- 不要发到 IM 工具里

---

## 5. 在 GitHub 仓库配置 Secrets

打开 <https://github.com/diaojz/cc-doctor/settings/secrets/actions>，逐项 **New repository secret**：

| Name | Value |
|---|---|
| `APPLE_CERTIFICATE` | 第 1.3 步剪贴板内容 |
| `APPLE_CERTIFICATE_PASSWORD` | 第 1.2 步设置的 .p12 导出密码 |
| `KEYCHAIN_PASSWORD` | 自定义任意强口令（如 `openssl rand -base64 24` 生成一个） |
| `APPLE_ID` | 你的 Apple ID 邮箱 |
| `APPLE_PASSWORD` | 第 2 步生成的 App-Specific Password |
| `APPLE_TEAM_ID` | 第 3 步的 10 位 Team ID |
| `TAURI_SIGNING_PRIVATE_KEY` | 第 4.1 步剪贴板内容 |

填完应该有 **7 项**（`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 本项目不需要）。

---

## 6. 触发首次签名发布

```bash
# 假设要发 v3.14.2
git checkout main         # 或者你想发布的分支
git pull
git tag v3.14.2
git push origin v3.14.2
```

Workflow `.github/workflows/release.yml` 会自动跑：

1. 在 macos-14 / windows-2022 / ubuntu-22.04 / ubuntu-22.04-arm 四个 runner 并行打包
2. macOS 端：
   - `pnpm tauri build --target universal-apple-darwin` 出 universal `.app`
   - Tauri 内置自动用 Developer ID Application 签名 + 通过 `notarytool` 公证 + `stapler staple`
   - 用 `create-dmg` 把 `.app` 包成带背景图的 `.dmg`，再独立公证一次 + staple
   - `codesign --verify` / `spctl` / `stapler validate` 三连验证
3. 产出上传到 GitHub Release
4. `latest.json` 自动生成（updater 配置）

成功后访问 <https://github.com/diaojz/cc-doctor/releases> 查看产出。

---

## 7. 验证产出（用户视角）

下载 `CC-Doctor-vX.Y.Z-macOS.dmg`，双击挂载，把 `CC Doctor.app` 拖入 Applications。第一次启动应该**直接打开**，不出现：

- ❌ "无法打开'CC Doctor'，因为它来自身份不明的开发者"
- ❌ "macOS 无法验证此 App 是否包含恶意软件"

你也可以本地手动验证：

```bash
spctl -a -t exec -vv /Applications/CC\ Doctor.app
# 期望：accepted, source=Notarized Developer ID
codesign -dv --verbose=4 /Applications/CC\ Doctor.app | grep Authority
# 期望第一行：Authority=Developer ID Application: 你的名字 (TEAM_ID)
xcrun stapler validate /Applications/CC\ Doctor.app
# 期望：The validate action worked!
```

---

## 8. 常见错误排查

### 8.1 `❌ TAURI_SIGNING_PRIVATE_KEY Secret 为空或不存在`

检查 GitHub Settings → Secrets and variables → **Actions**（不是 Codespaces / Dependabot）下是否有该 Secret。

### 8.2 `❌ No 'Developer ID Application' identity found`

证书类型选错了——你导出的可能是 Mac Development 或 Mac App Distribution。回到 §1.1 确认类型为 **Developer ID Application**。

### 8.3 公证失败 `Invalid` / `Status: Invalid`

通常原因：

- App-Specific Password 不对/过期 → 重新生成
- Apple ID 没接受最新开发者协议 → 登录 <https://developer.apple.com/account> 接受
- Team ID 写错 → 在开发者后台 Membership 页面再确认一次
- 二进制中夹带了未签名的 native 库 → 看 `notarytool log <submission-id>` 的具体报错

### 8.4 `latest.json` 生成空 platforms

某个平台 `.sig` 文件没产出，说明 `TAURI_SIGNING_PRIVATE_KEY` 没注入成功——回看 `Prepare Tauri signing key` step 的日志。

### 8.5 Gatekeeper 仍然拦截

确认这 4 步全部成功（看 workflow 日志）：

1. ✅ `xcrun stapler staple` 在 `.app` 上跑过
2. ✅ DMG 也单独公证 + staple 过
3. ✅ `spctl -a -t exec` 验证 `accepted`
4. ✅ `xcrun stapler validate` 验证通过

如果日志全绿但用户还是被拦——确认用户是从你的 GitHub Release 直链下载的，而不是先经过某个去除扩展属性的中转工具。

---

## 9. 后续日常发布

之后每次发版只要：

```bash
# bump 版本号
# - package.json
# - src-tauri/tauri.conf.json
# - src-tauri/Cargo.toml
# 三处保持一致
git commit -am "chore: bump v3.14.3"
git tag v3.14.3
git push && git push origin v3.14.3
```

CI 全自动签名 + 公证 + 上传 release。
