# 统一 Codex 会话历史

## 功能说明

Codex 会在每条已保存会话上记录供应商与模型。恢复旧会话时，这些值可能覆盖
当前 `config.toml`。如果切换过程没有同步这些状态，旧官方会话可能在 CC Switch
已经选中第三方后仍请求 OpenAI，旧第三方会话也可能引用 live 配置里已经不存在
的路由。

开启 **统一 Codex 会话历史** 后，CC Switch 会在每次切换时把已保存会话重新绑定
到当前选中的供应商，支持全部方向：

- 官方切到第三方；
- 第三方切回官方；
- 不同第三方供应商之间切换。

官方流量仍使用 Codex 内建的 `openai` 供应商，第三方流量仍使用其配置中的
供应商 ID。本功能不会再把官方流量伪装成 `custom`。

## 开启方式

1. 打开 **设置 > Codex 应用增强**。
2. 开启 **统一 Codex 会话历史**。
3. 可选勾选 **立即同步所有现有会话**。

勾选后会立即把现有会话对齐到当前供应商；不勾选也不会影响后续切换，开关保持
开启期间，每次供应商切换都会重新绑定已保存会话。

切换后请重新打开已经在运行的会话。CC Switch 可以更新磁盘文件和索引，但无法
替换另一个正在运行的 Codex 进程已经加载到内存里的路由。

## 关闭方式

关闭后，后续供应商切换不再重新绑定已保存会话；现有会话保留在最后一次选中的
供应商下。

旧版 CC Switch 可能在
`~/.cc-switch/backups/codex-official-history-unify-v1/` 留有精确还原备份。
检测到这种旧备份时，关闭弹窗会提供兼容还原选项。当前事务式协调器不会创建
迁移备份。

## 凭据安全

CC Switch 会让凭据始终归属于自己的路由：

- ChatGPT OAuth 保留在 `auth.json`；
- 第三方 API key 只写入该供应商表的局部 bearer token；
- 第三方路由激活时，全局 `auth.json` 不保留字符串 API key；
- 保留完整的非活动供应商定义，避免已经打开的会话在切换期间报
  `Model provider ... not found`。

这样可以阻止第三方 key 被发送到 `api.openai.com`，也可以阻止官方 OAuth 被当作
第三方凭据使用。

## 数据安全

协调器只更新以下路由字段：

- 活动与归档 JSONL 中的 `session_meta.payload.model_provider`；
- `thread_settings_applied` 中的供应商与模型；
- 所有已发现状态库中 `threads.model_provider`，以及目标明确配置模型时的
  `threads.model`。

写入前会先验证全部文件和数据库，并在内存中准备精确变更日志。JSONL 使用原子
写入并恢复原修改时间；SQLite 使用事务和旧值条件更新。如果任一步失败或发现
并发修改，已经应用的历史变更与 live 文件会在切换报错前按相反顺序回滚。

对话消息、响应条目和加密推理内容不会被删除或改写。

## 状态数据库位置

CC Switch 会扫描以下位置内的全部 `state_*.sqlite`：

- Codex 配置目录；
- 配置目录下的 `sqlite/`；
- `config.toml` 中的 `sqlite_home`；
- `CODEX_SQLITE_HOME`。

路径会规范化并去重，因此 `state_5.sqlite` 升级为 `state_6.sqlite` 等版本变化不会
导致部分历史被漏掉。

## 已知限制

`encrypted_content` 可能与生成它的后端绑定。重新绑定可以让会话出现在当前列表
并让下一次请求走当前供应商，但另一个后端仍可能无法解密原后端生成的推理内容。
CC Switch 不会也无法转换这部分加密内容。
