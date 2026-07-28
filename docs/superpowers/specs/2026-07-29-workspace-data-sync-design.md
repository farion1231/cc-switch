# cc-switch 工作数据备份、合并与 Cursor 支持设计

- 日期：2026-07-29
- 状态：已确认设计
- 目标版本：分阶段交付
- 涉及客户端：Claude Code、Codex、Grok Build、OpenCode、Cursor

## 1. 背景

cc-switch 当前已经具备以下基础能力：

1. WebDAV 与 S3 云端同步；当前同步对象为 cc-switch 数据库导出 `db.sql` 和 `skills.zip`。
2. Session Manager；当前已经支持 Claude、Codex、Grok Build、OpenCode 等客户端的本地会话扫描和查看。
3. 部分客户端配置目录、SQLite 状态库和会话格式探测能力。

当前云端同步采用整份快照上传和下载覆盖的方式，不支持 Claude、Codex、Grok Build、OpenCode、Cursor 原生工作数据的增量备份、跨设备合并和冲突保留。Cursor 尚未接入 Session Manager。

本设计在不破坏现有配置和 Skills 同步协议的前提下，增加独立的工作数据同步系统。

## 2. 已确认的产品决策

### 2.1 同步范围

同步以下工作数据：

- Session：会话、消息、工具调用和必要附件。
- Task/Todo：任务和待办事项。
- Plan/Goal：计划和目标。
- Memory：客户端生成或管理的记忆数据。
- 必要的会话索引和关联元数据。

默认不备份普通客户端配置。未来可以增加非敏感配置的显式可选备份，但不属于首期目标。

### 2.2 凭据边界

以下数据永不进入工作数据备份：

- `auth.json`。
- API Key。
- OAuth access token 和 refresh token。
- Cookie 和 session token。
- 密码和 client secret。
- 设备认证信息。
- 供应商配置中的认证字段。

### 2.3 Cursor 首期范围

Cursor 首期只实现：

- Agent、Chat 和 Composer 数据的扫描、备份、合并和恢复。
- Rules/Memory 数据同步。
- Cursor 会话接入 Session Manager。

首期不实现 Cursor 模型供应商或 API 配置切换，不增加 `AppType::Cursor`。

### 2.4 冲突策略

采用无损优先策略：

- 能证明安全时自动合并。
- 不能证明安全时保留两个版本或进入冲突仓库。
- 不使用“更新时间较新”作为静默覆盖另一份数据的充分条件。
- 不允许冲突数据无记录丢失。

### 2.5 加密策略

工作数据默认启用客户端侧端到端加密：

- 用户设置独立同步密码。
- 密码通过 Argon2id 派生主密钥。
- 数据使用 XChaCha20-Poly1305 加密。
- WebDAV/S3 只保存密文和非敏感的加密引导参数。
- 密码可以保存到系统钥匙串，但不写入 cc-switch 配置或云端。
- 丢失同步密码后无法恢复云端工作数据。
- 用户可以在高级设置中关闭加密，但必须确认风险。
- 加密与非加密数据使用不同的远端 profile namespace；切换加密模式必须创建新 profile 或执行显式迁移，不能在原 profile 中混用密文和明文对象。

## 3. 非目标

首期不实现：

- Cursor 模型供应商切换。
- 团队多人共享和权限管理。
- 云端服务端解析会话内容。
- 所有未知客户端版本的自动写回。
- 首个稳定版本即默认开启完全自动双向写入。
- 跨客户端格式转换，例如将 Claude 会话恢复成 Codex 会话。
- 对用户整个项目目录或源码仓库进行备份。

## 4. 方案选型

### 4.1 不采用：整个目录压缩上传

直接备份 `~/.claude`、`~/.codex`、`~/.grok`、OpenCode 数据目录和 Cursor 数据目录实现简单，但存在以下问题：

- 无法可靠合并。
- 容易包含凭据、日志和缓存。
- SQLite 和 WAL 可能不一致。
- Cursor 和 Codex 目录可能非常大。
- 恢复时只能覆盖，破坏风险高。

### 4.2 不采用：只保存统一规范化模型

把所有原生数据转换成 cc-switch 自定义格式便于搜索和比较，但无法保证工具调用、分支、快照、附件和客户端内部状态无损回写。

### 4.3 采用：原生数据包、统一清单与 Provider 合并适配器

每个 Provider Adapter 负责：

1. 探测客户端和数据格式。
2. 生成数据清单。
3. 排除凭据和无用文件。
4. 创建一致性快照。
5. 导出可合并记录和必要原生数据。
6. 生成三方合并计划。
7. 事务应用合并结果。
8. 对不能安全处理的数据降级为只读备份或冲突副本。

## 5. 总体架构

```text
UI
  -> Workspace Sync Commands
    -> Workspace Sync Engine
      -> Provider Adapters
        -> Claude
        -> Codex
        -> Grok Build
        -> OpenCode
        -> Cursor
      -> Snapshot / Merge / Conflict / Tombstone
      -> Crypto / Blob Store
      -> Object Storage
        -> WebDAV
        -> S3
```

建议新增模块：

```text
src-tauri/src/workspace_sync/
├── mod.rs
├── engine.rs
├── model.rs
├── inventory.rs
├── snapshot.rs
├── manifest.rs
├── merge.rs
├── conflict.rs
├── tombstone.rs
├── transaction.rs
├── restore_point.rs
├── security.rs
├── crypto.rs
├── blob_store.rs
├── device.rs
├── gc.rs
├── error.rs
├── progress.rs
├── storage/
│   ├── mod.rs
│   ├── webdav.rs
│   ├── s3.rs
│   └── memory.rs
└── adapters/
    ├── mod.rs
    ├── common.rs
    ├── claude.rs
    ├── codex.rs
    ├── grokbuild.rs
    ├── opencode.rs
    └── cursor/
        ├── mod.rs
        ├── detect.rs
        ├── global_storage.rs
        ├── workspace_storage.rs
        ├── merge.rs
        └── schema_registry.rs
```

## 6. Provider Adapter 接口

概念接口如下：

```rust
trait WorkspaceDataAdapter {
    fn provider_id(&self) -> &'static str;
    fn detect(&self) -> Result<DetectionResult>;
    fn inventory(&self) -> Result<Vec<DataItem>>;
    fn create_snapshot(&self, selection: &BackupSelection)
        -> Result<ProviderSnapshot>;
    fn inspect_remote(&self, snapshot: &ProviderSnapshot)
        -> Result<SnapshotInspection>;
    fn plan_merge(
        &self,
        base: Option<&ProviderSnapshot>,
        local: &ProviderSnapshot,
        remote: &ProviderSnapshot,
    ) -> Result<MergePlan>;
    fn apply_merge(&self, plan: &MergePlan) -> Result<ApplyResult>;
}
```

现有 Session Manager Provider 解析器继续负责只读展示。可将通用解析代码抽取给 Workspace Adapter 使用，但不把事务写回逻辑直接放入现有 Session Manager 解析器。

## 7. 统一数据模型

```rust
struct DataItem {
    provider: WorkspaceProviderId,
    kind: DataKind,
    logical_id: String,
    parent_id: Option<String>,
    native_path: String,
    content_hash: String,
    updated_at: Option<i64>,
    schema_fingerprint: Option<String>,
    merge_capability: MergeCapability,
    sensitivity: Sensitivity,
}
```

`DataKind` 至少包含：

- Session
- Task
- Todo
- Plan
- Goal
- Memory
- Index
- Attachment

`MergeCapability` 至少包含：

- `AppendOnly`
- `RecordSet`
- `Text`
- `Opaque`
- `Unsupported`

## 8. 云端协议

现有配置和 Skills 同步目录保持不变。工作数据使用独立目录：

```text
cc-switch-sync/workspace-v1/<profile>/
├── bootstrap.json
├── head.enc
├── snapshots/
│   └── <snapshot-id>.enc
├── blobs/
│   └── <encrypted-object-id>.blob
└── devices/
    └── <device-id>.enc
```

### 8.1 bootstrap

`bootstrap.json` 是唯一明文对象，只包含：

- 协议格式和版本。
- 加密算法。
- KDF 算法、salt 和参数。
- 密钥校验数据。

不包含设备名、Provider、会话数量、路径、文件名和原文哈希。关闭加密时，bootstrap 明确记录 `encryption.mode = none`，对象使用独立 namespace；该模式不得读取或覆盖加密 profile。

### 8.2 Snapshot DAG

Snapshot 是不可变对象，支持多个父节点：

```text
Device A Snapshot ---\
                       -> Merge Snapshot
Device B Snapshot ---/
```

Manifest 包含：

- snapshot ID。
- parent snapshot IDs。
- 创建时间和设备 ID。
- Provider adapter 版本。
- 原生客户端版本。
- schema fingerprint。
- DataItem 清单。
- Tombstone 清单。
- 统计信息。

### 8.3 内容寻址 Blob

```text
plaintextHash = SHA256(content)
objectId = HMAC(ObjectIdKey, protocolVersion || profileId || providerId || dataKind || plaintextHash)
blobKey = HKDF(BlobEncryptionKey, objectId)
```

相同 profile、Provider 和数据类型中的相同内容得到相同 object ID，以支持增量上传和 Blob 去重。Provider/数据类型参与域分离，避免同一密文对象在不同认证上下文中复用；远端无法直接获得原文 SHA-256。

### 8.4 云端 Head 并发更新

同步顺序：

1. 拉取 Head 和 ETag。
2. 上传缺失 Blob。
3. 上传不可变 Snapshot。
4. 使用 `If-Match` 条件写入 Head。
5. 条件写入失败时重新拉取新 Head。
6. 重新进行三方合并并创建 Merge Snapshot。
7. 有限次数重试。

WebDAV 不支持条件请求时降级为有过期时间的远端租约锁，并在 UI 中显示兼容模式。

## 9. 端到端加密

### 9.1 密钥层级

```text
Sync Password
  -> Argon2id
    -> Master Key
      -> HKDF Manifest Key
      -> HKDF Blob Encryption Key
      -> HKDF Object ID Key
      -> HKDF Key Check Key
```

### 9.2 密钥存储

- 优先使用系统钥匙串。
- 设置中只保存 `credentialRef`。
- 用户可选择不保存密码。
- 密钥和密码不写日志或错误详情。
- 内存敏感字节使用 `zeroize` 清理。

### 9.3 建议依赖

- `argon2`
- `chacha20poly1305`
- `hkdf`
- `zeroize`
- `rand`
- 跨平台系统钥匙串库

## 10. 三方合并模型

每台设备保存上次成功同步的 Base Snapshot：

- Base：双方上次共同确认的版本。
- Local：当前本地状态。
- Remote：当前云端状态。

规则：

- 只有 Local 改变：上传。
- 只有 Remote 改变：应用远端变化。
- 两边新增不同项目：取并集。
- 同一项目两边变化：Provider Adapter 合并。
- 无法安全合并：创建冲突副本或进入冲突仓库。

合并成功并完成本地验证后，必须创建新的 Merge Snapshot 并上传。若云端 Head 在此期间变化，则重新合并后再更新。

## 11. 删除与 Tombstone

删除不能只移除本地文件，否则旧云端数据会重新出现。

Tombstone 包含：

- provider。
- kind。
- logical ID。
- last known hash。
- deleted at。
- deleted by device。

规则：

- Tombstone 比记录更新且另一端未继续修改时，执行删除。
- 一端删除、另一端继续修改时，创建删除冲突。
- Tombstone 默认保留 30 天。
- 活跃设备确认后才能回收对应 Blob。
- 被明确移除或长时间不活跃的设备不继续阻塞 GC。

## 12. Claude Adapter

### 12.1 范围

从项目已有 Claude 配置目录探测逻辑开始，白名单探测：

- `projects/` 中的会话 JSONL 和 sidecar。
- `plans/`。
- `tasks/`。
- `todos/`。
- 通过格式识别发现的 memory 数据。

不假定所有版本拥有相同目录。未知格式允许备份但禁止自动写回。

### 12.2 会话合并

- 哈希相同：去重。
- 一份 JSONL 是另一份完整前缀：保留较长版本。
- 共同前缀后发生分叉：保留两个会话分支，不拼接事件流。
- 冲突副本记录 `forkedFrom` 和来源设备。

### 12.3 Task、Plan 和 Memory

- 不同逻辑 ID：取并集。
- 单边变化：采用变化版本。
- 文本 Plan：执行 Base/Local/Remote 三方文本合并。
- 同一区域双边修改：生成冲突文件。
- 无稳定 ID 或更新时间的结构化数据：保留两个版本。

## 13. Codex Adapter

### 13.1 范围

按当前有效 Codex 数据目录和数据库路径探测：

- `sessions/**/*.jsonl`。
- `archived_sessions/**/*.jsonl`。
- `session_index.jsonl`。
- 活跃状态数据库中的会话相关表。
- `goals_*.sqlite`。
- `memories_*.sqlite`。
- `memories/`。

排除：

- `auth.json`。
- 日志数据库。
- `.tmp`。
- plugins 缓存。
- computer-use 缓存和录屏。
- IPC、锁文件和进程状态。
- 模型缓存和设备标识。

`shell_snapshots` 首期默认不备份。未来可在确认会话引用关系后作为高级选项按引用同步。

### 13.2 SQLite 快照

使用项目已经启用的 `rusqlite` Backup API 创建一致性快照，不直接复制活跃 SQLite 文件。

导出逻辑记录，包括当前 schema 中与以下概念对应的表：

- threads。
- thread dynamic tools。
- thread spawn edges。
- goals。
- memories。

具体表名和字段由 schema adapter 决定。

### 13.3 合并

- Rollout JSONL 使用前缀和分叉规则。
- `threads` 按 ID 合并。
- 分叉时生成新 thread ID，并重写 rollout path 和关联表引用。
- Goal 状态冲突不自动用 `complete` 覆盖 `active`。
- Memory 按稳定键、source update 信息和内容哈希合并。
- 写回前提示关闭 Codex CLI/Desktop。
- 所有 SQLite 更新在事务中完成并执行完整性检查。

## 14. Grok Build Adapter

### 14.1 范围

按现有 Session Manager 已支持的布局探测：

- `sessions/`。
- `archived_sessions/`。
- session 目录中的 `summary.json`。
- `chat_history.jsonl`。
- 与会话绑定的已识别附件和状态文件。

### 14.2 合并

- `summary.info.id` 作为 session ID。
- 不同 session ID：取并集。
- history 相同：去重。
- 一份 history 是另一份前缀：保留较长版本。
- 分叉：复制 session 目录、生成新 ID 并重写 summary 引用。
- 未识别文件作为 opaque attachment 跟随会话。

## 15. OpenCode Adapter

### 15.1 范围

支持：

- XDG 数据目录或 `~/.local/share/opencode/`。
- 新版 SQLite 存储。
- 旧版 `storage/` JSON 存储。

逻辑导出以下概念对应的记录：

- project。
- project directory。
- workspace。
- session。
- session message 或 message/part。
- session input。
- todo。
- session context。

`snapshot/` 可能包含项目源代码，默认不备份。用户显式开启时按内容哈希去重并给出敏感内容提示。

### 15.2 合并

SQLite 记录按主外键拓扑顺序处理：

```text
project
  -> workspace
  -> session
  -> message/session_message
  -> part
  -> todo
  -> context
```

- 按稳定主键合并。
- 同一消息 ID 内容不同时视为分叉。
- 分叉时复制 session 并重写所有子表 session ID。
- Todo 随各自分支保留。
- 优先使用经验证能够完整保留数据的客户端 export/import 能力，否则使用 schema adapter。
- 写回后执行 `PRAGMA integrity_check` 和 `PRAGMA foreign_key_check`。

## 16. Cursor Adapter

### 16.1 数据目录

支持：

- `~/.cursor/` 中白名单用户数据。
- macOS：`~/Library/Application Support/Cursor/User/`。
- Windows：`%APPDATA%\Cursor\User\`。
- Linux：`~/.config/Cursor/User/`。

### 16.2 数据范围

选择性导出：

- `globalStorage/state.vscdb` 中的 Composer header。
- 对应 Composer 的 Bubble/Message。
- 被会话引用的 agent Blob。
- checkpoint 和 subagent 关系。
- workspace 与 Composer 的关联。
- `workspaceStorage/*/state.vscdb` 中的旧版 Chat/Composer 数据。
- 用户级 Rules/Memory。
- 被会话关联或用户显式选择项目中的 `.cursor/rules/`。

不备份整个 Cursor globalStorage 数据库，不备份未被会话引用的 Blob。

### 16.3 排除

- Token 和 Cookie。
- machine ID。
- Network Persistent State。
- Extension 缓存。
- GPUCache 和 Code Cache。
- Crashpad、日志和 telemetry。
- Service Worker 缓存。
- 项目源码全文。

### 16.4 Schema Registry

通过以下信息计算 schema fingerprint：

- SQLite 表集合。
- `PRAGMA user_version`。
- 字段签名。
- 关键 key 前缀。
- Cursor 应用版本。

每个 schema adapter 声明：

- can read sessions。
- can write sessions。
- can merge bubbles。
- can rewrite composer ID。
- can restore rules。

未知 schema 的行为：

- 允许读取时执行备份和 Session Manager 展示。
- 禁止不安全写回。
- 远端待恢复数据进入本地冲突仓库。

### 16.5 合并

- 不同 Composer ID：取并集。
- 相同 Composer ID、Bubble 主键不冲突：按 Bubble ID 和关系重建时间线。
- 相同 Bubble ID 内容不同时视为分叉。
- schema 支持安全重写时复制 Composer、生成新 ID 并重写所有引用。
- schema 不支持时保留本地版本，远端版本进入冲突仓库。

## 17. 冲突仓库

目录：

```text
<cc-switch-data-dir>/workspace-sync/conflicts/
└── <provider>/
    └── <conflict-id>/
        ├── conflict.json
        ├── local/
        └── remote/
```

Session Manager 和冲突中心展示：

- 正常会话。
- 冲突副本。
- 仅云端数据。
- 等待写回。
- 当前客户端版本不支持恢复的数据。

Session 冲突默认动作是“两个版本都保留”。

## 18. 本地事务与回滚

同步写入流程：

1. Preflight。
2. 创建本地恢复点或 before-image。
3. 在临时位置构建合并结果。
4. 校验临时结果。
5. 提示关闭目标客户端或取得安全写入条件。
6. 原子写入或事务写入。
7. 重新扫描并验证。
8. 记录本地事务成功。
9. 创建并上传 Merge Snapshot。

### 18.1 Preflight

检查：

- 磁盘空间。
- 同步密码和远端数据认证。
- Provider schema 写回能力。
- 客户端运行状态。
- 未完成同步事务。
- 对象数量和大小限制。
- 路径合法性。

### 18.2 文件写入

使用临时文件、fsync 和原子 rename，不直接截断原文件。

### 18.3 SQLite 写入

- Backup API 创建一致性副本。
- 在副本验证 Merge Plan。
- 执行 integrity 和 foreign key 检查。
- 行级事务写回，或在客户端关闭后原子替换。
- Cursor 大型数据库优先使用行级 before-image，不复制整个数据库作为恢复点。恢复点默认保留最近 10 次或 30 天，达到任一清理条件后按最旧优先删除；存在未完成事务时对应恢复点不得清理。

### 18.4 崩溃恢复

事务状态至少包含：

- preparing
- downloading
- planning
- awaiting_confirmation
- applying
- verifying
- uploading
- completed
- rolled_back
- failed

启动时检查未完成事务并选择安全继续或回滚。

## 19. 独立本地数据库

工作数据同步使用独立数据库：

```text
<cc-switch-data-dir>/workspace-sync/workspace-sync.db
```

不将同步事务、设备 ID、Credential reference 和 Blob 引用加入 cc-switch 主数据库，避免它们被现有 `db.sql` 再次同步。

独立数据库包含：

- sync transactions。
- per-provider results。
- conflicts。
- tombstones。
- devices。
- snapshot cache。
- blob refs。
- provider schema cache。

独立数据库有自己的 schema version。首期为 version 1，不修改主数据库当前 schema version。

## 20. Object Storage 抽象

统一接口至少支持：

```rust
trait ObjectStorage {
    async fn get(&self, key: &str) -> Result<Option<RemoteObject>>;
    async fn put(&self, key: &str, data: ByteStream) -> Result<PutResult>;
    async fn put_if_match(
        &self,
        key: &str,
        etag: &str,
        data: ByteStream,
    ) -> Result<ConditionalPutResult>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

修改底层 WebDAV/S3 transport，但保留现有 `webdav_sync.rs` 和 `s3_sync.rs` 的公开行为。

## 21. 安全控制

### 21.1 双重过滤

- 路径和表白名单。
- 结构化字段和内容敏感信息扫描。

敏感检测分为：

- block：禁止上传。
- redact：从备份副本中移除结构化字段。
- warn：会话正文可能包含密钥，由用户决定阻止、脱敏或确认加密上传。

### 21.2 文件系统防护

防止：

- path traversal。
- symlink escape。
- ZIP Slip。
- 软链接循环。
- 特殊设备文件。
- 压缩炸弹。
- 超大 Manifest。
- 超大对象和对象数量攻击。

建议默认限制：

- Manifest：16 MB。
- 单结构化记录：16 MB。
- 单普通 Blob：256 MB。
- 单次同步：5 GB。
- 最大解压比例：100:1。
- 最大条目数：1,000,000。

## 22. UI 设计

设置中分离：

```text
云端存储
配置与 Skills 同步
工作数据同步
```

WebDAV/S3 凭据可以复用，但两种同步拥有独立协议、状态和操作。

### 22.1 工作数据同步页

展示：

- 云端连接和加密状态。
- 当前设备。
- 最近同步时间。
- 云端快照。
- 待上传、待下载和冲突数量。
- Provider 数据数量和预计大小。
- Provider schema 读写能力。

操作：

- 扫描本地数据。
- 预览同步。
- 立即备份。
- 从云端合并。
- 双向同步。
- 查看历史快照。
- 管理设备。
- 冲突中心。

### 22.2 同步预览

按 Provider 展示：

- 新增。
- 更新。
- 删除。
- 自动合并。
- 冲突。
- 预计传输大小。

### 22.3 自动同步

首期策略：

- 支持手动双向同步。
- 支持可选定时备份。
- 启动时只检查远端变化，不自动写入客户端。
- 退出时可自动上传本地新增数据。
- 冲突、删除和 schema 变化必须人工确认。

## 23. Session Manager 改造

新增 Cursor Provider 解析器，并扩展 SessionMeta：

- sync state。
- conflict ID。
- origin device。
- remote snapshot ID。
- native write-back supported。
- forked from。

Session Manager 不直接访问网络，只聚合：

- Provider 本地扫描结果。
- workspace-sync.db 的本地同步状态。
- 本地冲突仓库。

Provider 过滤列表改为动态生成，不继续硬编码。

## 24. 后端改造清单

新增：

- `src-tauri/src/workspace_sync/`。
- `src-tauri/src/commands/workspace_sync.rs`。
- `src-tauri/src/session_manager/providers/cursor.rs`。

修改：

- `src-tauri/src/lib.rs`。
- `src-tauri/src/commands/mod.rs`。
- `src-tauri/src/settings.rs`。
- `src-tauri/src/services/webdav.rs`。
- `src-tauri/src/services/s3.rs`。
- `src-tauri/src/session_manager/mod.rs`。
- `src-tauri/src/session_manager/providers/mod.rs`。
- `src-tauri/src/commands/session_manager.rs`。
- `src-tauri/Cargo.toml`。

建议 Commands：

- `workspace_sync_get_status`
- `workspace_sync_scan`
- `workspace_sync_preview`
- `workspace_sync_execute`
- `workspace_sync_cancel`
- `workspace_sync_unlock`
- `workspace_sync_lock`
- `workspace_sync_change_password`
- `workspace_sync_list_snapshots`
- `workspace_sync_restore_snapshot`
- `workspace_sync_list_conflicts`
- `workspace_sync_resolve_conflict`
- `workspace_sync_list_devices`
- `workspace_sync_remove_device`
- `workspace_sync_provider_diagnostics`
- `workspace_sync_create_restore_point`
- `workspace_sync_rollback`

## 25. 前端改造清单

新增：

```text
src/lib/api/workspaceSync.ts
src/lib/query/workspaceSyncQueries.ts
src/lib/query/workspaceSyncMutations.ts
src/hooks/useWorkspaceSyncProgress.ts
src/components/workspace-sync/
```

工作数据同步组件至少包含：

- WorkspaceSyncPage。
- SyncOverview。
- ProviderSyncCard。
- SyncPreviewDialog。
- SyncProgressDialog。
- ConflictCenter。
- ConflictDetail。
- SnapshotHistory。
- DeviceManager。
- EncryptionSetupDialog。
- ProviderDiagnostics。
- RestorePointList。

修改：

- `src/components/settings/SettingsPage.tsx`。
- `src/components/settings/WebdavSyncSection.tsx`。
- `src/components/sessions/SessionManagerPage.tsx`。
- `src/components/sessions/SessionItem.tsx`。
- `src/components/sessions/utils.ts`。
- `src/types.ts`。
- i18n locale 文件。

## 26. 实施阶段

### Phase 0：格式调研和 Fixtures

- 建立脱敏测试样本。
- 定义统一模型。
- 实现 Provider 探测和 capability matrix。
- 实现 Cursor schema registry 骨架。

预计 1～2 人周。

### Phase 1：加密备份协议

- Object Storage 抽象。
- WebDAV/S3 条件写入。
- 端到端加密。
- Blob 去重。
- Snapshot DAG。
- 设备管理。
- 所有目标 Provider 的只读备份。

预计 2～3 人周。

### Phase 2：Claude/Grok 文件合并

- JSONL 前缀和分叉检测。
- 文本三方合并。
- Tombstone。
- 恢复点和原子写入。
- 合并后自动上传 Merge Snapshot。

预计 2 人周。

### Phase 3：Codex/OpenCode SQLite 合并

- Online Backup。
- schema adapter。
- 逻辑记录导出。
- ID 重写。
- 外键和完整性检查。
- 事务写回和崩溃恢复。

预计 3～4 人周。

### Phase 4：Cursor

- Cursor Session Manager 读取。
- Composer、Bubble 和 Blob 关联。
- 旧版 workspace 数据读取。
- schema 已知版本的安全写回。
- Rules/Memory 合并。

预计 3～4 人周。

### Phase 5：自动同步和正式发布

- 定时增量备份。
- 启动检查和退出上传。
- Snapshot 历史恢复。
- 设备移除。
- Tombstone 和 Blob GC。
- 跨平台、安全和性能验证。

预计 2～3 人周。

单人总计约 13～18 人周。两人合理并行预计约 8～11 个自然周。

## 27. 推荐 PR 拆分

1. Storage Transport。
2. Crypto 和 Snapshot Protocol。
3. Inventory 和 Backup-only Adapters。
4. Sync Engine、Conflict、Tombstone 和 Transaction。
5. Claude/Grok Write-back。
6. Codex/OpenCode Write-back。
7. Cursor Session Manager。
8. Cursor Write-back。
9. UI、Auto Backup 和 Hardening。

## 28. 测试策略

### 28.1 Merge 性质测试

验证：

- 幂等性。
- 无损性。
- 无冲突情况下的顺序稳定。
- 相同输入得到相同内容 Snapshot ID。
- 所有未进入结果的数据都有明确 Conflict 或 Tombstone 记录。

### 28.2 加密测试

验证：

- 正确密码解密。
- 错误密码失败。
- 密文和 Manifest 篡改失败。
- object ID 不暴露原文 SHA-256。
- 相同内容去重。
- 不同 profile 不能互相解密。
- 密钥不进入日志和错误消息。

### 28.3 Provider 测试

覆盖：

- 空目录和自定义路径。
- 旧版、当前版和未知格式。
- 损坏 JSON、JSONL 和 SQLite。
- WAL 数据库。
- 大会话和会话分叉。
- 同 ID 不同内容。
- symlink 和路径逃逸。
- 敏感文件排除。
- ID 重写后的引用完整性。

### 28.4 多设备测试

模拟：

- 不同设备新增不同会话。
- 同一会话双边继续导致分叉。
- 一端删除、另一端未修改。
- 一端删除、另一端继续修改。
- 两端同时 CAS 更新 Head。

### 28.5 故障注入

在以下位置模拟崩溃：

- Blob 上传中。
- Snapshot 上传完成但 Head 未更新。
- 恢复点创建后。
- SQLite 应用中。
- 本地写入完成但验证未完成。
- 本地完成但 Merge Snapshot 上传失败。

系统重启后必须安全继续或回滚。

## 29. 性能目标

- 增量扫描 10 万数据项目标小于 10 秒。
- 处理 1 GB 备份峰值内存目标小于 256 MB。
- Blob 使用流式哈希、流式加密和流式上传。
- 本地无变化时不重复上传 Blob，控制数据传输目标小于 2 MB。
- Cursor 使用 SQL 前缀查询、游标分页和引用驱动的 Blob 读取，不将整个数据库载入内存。

## 30. 验收标准

### 30.1 功能

- 五个目标客户端均可扫描和生成备份预览。
- WebDAV/S3 均可上传和下载加密快照。
- 新设备输入密码后可以读取云端数据。
- 合并成功后自动上传合并结果。
- Session Manager 可以查看 Cursor 会话。
- Claude、Codex、Grok Build、OpenCode 对已支持格式能够安全写回。
- Cursor 已知 schema 能够写回，未知 schema 明确降级。

### 30.2 数据安全

- 凭据不进入备份。
- 分叉不静默覆盖。
- 同步失败可回滚。
- 删除后新修改不会被 Tombstone 静默删除。
- 所有写入都有恢复点或 before-image。
- 密文认证失败时不写入本地。

### 30.3 兼容性

- 现有 `db.sql + skills.zip` 同步行为不变。
- 现有 Session Manager 不回归。
- 未启用工作数据同步的用户行为不变。
- macOS、Windows 和 Linux 路径探测正确。
- 自定义 Claude/Codex 配置目录继续生效。

### 30.4 工程质量

以下检查全部通过：

```text
pnpm typecheck
pnpm format:check
pnpm test:unit
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

并增加工作数据同步多设备集成测试和安全测试。

## 31. 发布策略

建议版本节奏：

1. Alpha：加密备份和云端快照浏览。
2. Beta 1：Claude/Grok 双向合并。
3. Beta 2：Codex/OpenCode 双向合并。
4. RC：Cursor 写回和完整冲突中心。
5. Stable：定时同步、历史恢复和垃圾回收。

工作数据同步在 Alpha/Beta 阶段由实验性开关控制。Provider 写回能力分别启用，不使用一个全局开关一次性开放全部写回。

## 32. 最终产品定义

cc-switch 提供面向 Claude、Codex、Grok Build、OpenCode 和 Cursor 的加密、增量、无损优先的工作数据备份与多设备合并。系统自动合并能够证明安全的数据；无法证明安全时保留冲突副本，绝不静默覆盖。客户端格式未知时降级为只读备份和 Session Manager 查看。
