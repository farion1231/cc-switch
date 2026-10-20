# 测试与验收策略

日期：2026-10-19；执行记录更新：2026-10-20。本文件区分已执行的工程验证与未来完整安全验收。V08/V14 的部分回归用例已实现，整体矩阵尚未通过验收。

## 本轮整理验证

| 检查 | 结果 |
| --- | --- |
| `pnpm typecheck` | 通过，退出码 0 |
| `pnpm build:renderer` | 通过，退出码 0，3463 个模块；未运行桌面程序 |
| 构建警告 | 浏览器兼容数据过期、Tailwind duration 歧义、静态/动态混合导入及大 chunk；本轮未改运行时代码，不顺带升级依赖 |
| 删除来源 | 324 个文件删除前均与 HEAD 内容相同；路径、大小和 SHA-256 已记录 |
| 原始项目总结、LICENSE 与保留代码 | 通过：原始总结 SHA-256 与 Git index 条目不变；LICENSE 与 HEAD 一致；运行时源码、测试及构建依赖文件未修改，src 内仅调整 whats-new 的说明文档 |
| 文档链接与 diff 空白 | 新增文档与保留指南的本地文件目标已检查；历史资料引用改用固定 commit 的上游链接，未检查远端可达性；`git diff --check` 通过 |
| 全量前端单测、Rust 编译/单测/Clippy、桌面打包 | 本轮未运行；没有修改运行时实现，列为 P0 基线任务 |
| Agent E2E / 安全能力 | 未实现、未验证，不能从前端构建通过推断安全性 |

## P0-01 验证（2026-10-19）

本轮只改应用身份、更新渠道禁用、品牌文案/资产与对应测试，不改脱敏语义。在 macOS 本机、锁定 Node 22.12.0 + pnpm 10.12.3、rust-toolchain 1.95 下执行：

| 检查 | 结果 |
| --- | --- |
| `pnpm typecheck` | 通过，退出码 0 |
| `pnpm format:check` | 通过（i18n JSON 重写后仍符合 Prettier） |
| `pnpm test:unit` | 通过，231 个文件 / 3029 个用例全部通过（含同步更新的存储键、NewLayoutDialog/WhatsNew 负例） |
| `pnpm build:renderer` | 通过；警告与基线相同（浏览器兼容数据过期、chunk 体积），未顺带升级依赖 |
| `cargo fmt --check` | 通过（超宽品牌字符串由 rustfmt 自动换行） |
| `cargo check` / `cargo test --no-run` | 通过，全部集成测试目标可编译，无新增 warning |
| `cargo clippy -- -D warnings`（CI 同口径） | 通过 |
| `cargo test --lib` | 通过：3797 个单测（含 tray 菜单顺序、deeplink parser、backup、handlers、config 路径用例） |
| 定向集成测试 | `app_config_load` 4、`import_export_sync` 19、`mcp_commands` 33、`profile_roundtrip` 7、`skill_sync` 7、`provider_service` 51 全部通过 |
| nextest 全量（含 proxy/golden 套件） | 未运行（本机未装 cargo-nextest），归入 P0-04 基线回归 |
| 身份残留扫描 | 代码/配置中 `com.ccswitch.desktop`、`.cc-switch`、`ccswitch://`、旧更新端点均为 0 命中 |
| V14 双应用真机并存 | 未执行：静态隔离面（identifier/数据目录/scheme/更新渠道/托盘与单实例标识）已成立；安装打包并存 smoke 留待 P0-04 |
| Agent E2E / 安全能力 | 仍未实现；本轮未验证任何脱敏行为 |

本轮失败历史：首次 `pnpm test:unit` 44 个失败，均为身份改名后的测试键/按钮断言失配（存储键前缀、NewLayoutDialog 发布说明按钮、WhatsNew 外链断言），已同步修正后全量通过；Rust 未出现测试失败。无基线失败与新回归混洌问题（上轮未跑过这些套件，本轮首次建立前端单测 + lib 单测 + 定向集成三个层次的本地基线）。

## P0-02a 验证（2026-10-20）

实际初轮环境：Node 24.15.0、pnpm 10.12.3、Homebrew Rust 1.97.1；这与 `.node-version` 的 22.12.0 和 Rust 1.95 锁定配置不同。随后已使用完整 Rust 1.95 工具链路径复验 Clippy；最终后端回归结果见 P0-02b1。

代码范围：显式接管标识、退出/启动失败行为、未受管配置的模式启动保护，以及对应界面提示。未运行桌面程序，未接入真实 Agent 或模型服务。Rust 集成/单测使用既有临时 HOME 和本地测试服务器。

| 检查 | 结果 |
| --- | --- |
| `pnpm typecheck` | 通过，退出码 0 |
| `pnpm format:check` | 通过，退出码 0 |
| `pnpm test:unit` | 231 个文件、3029 个用例通过 |
| `pnpm build:renderer` | 通过；仍有基线浏览器数据、Tailwind 歧义、混合导入与大 chunk 警告 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 3803 通过、0 失败、9 忽略；忽略用例不计为通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test golden --test provider_commands --test provider_service` | 通过：golden 43、provider_commands 11、provider_service 51，共 105 项 |
| `cargo fmt --check`、`git diff --check` | 通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Rust/Clippy 1.95 下通过；1.97.1 对既有 `gemini_mcp.rs` 报 `for_kv_map`，未为适配非锁定版本改写该文件 |
| V08 / V14 | 仅覆盖下述自动化子集；双应用真机并存、kill/升级、全部启动副作用及完整安全运行时未验收 |

新增 6 个 Rust 单测覆盖：

- 上游 `PROXY_MANAGED` 配置没有 Shield 管理记录时，不被模式启动器改写。
- 正常退出保持客户端文件与设备状态的原始字节，包括外部编辑；真实 TCP 监听在退出后不可连接。
- 启动端口占用时，既有本地路由和占位符保持不变，不回填真实直连凭据。
- Codex 上游官方接管表在切换投影中保留。
- 上游 catalog 文件和官方接管 ID 不被判为 Shield 所有。
- Claude Desktop 上游 profile 文件和元数据条目在 Shield 配置应用/撤销后保留。

既有用例同步验证正常退出/异常退出后的账号恢复，以及 Desktop 接管失败保留路由。托盘用例改为检查“仍在代理模式”不会自动清除接管失败提示，只有成功重试或明确处理才清除。

调试中发现账号校验失败可能发生在共享监听启动之后：已改为任一启动接管失败时停止共享监听，而不是仅修改测试预期。其代价是本次启动中的其他受管 Agent 也暂不可用；逐 Agent 就绪门控仍属于后续安全运行时设计。首轮旧退出语义断言、后续共享监听断言和土耳其语字符串前缀断言失败均已修正并通过单测。

golden 初跑发现两处失配：新 catalog 文件名改变了排序，已核对后更新目录快照（权限仍为 600/644）；另一处用户表 fixture 已在基线改名而断言仍查旧名称，已通过 `git show HEAD` 确认是既有测试不一致，并修正断言。没有批量接受未知快照。

P0-02a 不覆盖端口与全部启动导入；后续进展见下节。共享 `custom` 表与完整恢复路径仍未隔离。

## P0-02b1 验证（2026-10-20）

在 P0-02a 基础上验证默认端口和启动初始化修改，增加手动导入按钮及对应测试。后端明确将 `/Users/xuzhongxin/.rustup/toolchains/1.95-aarch64-apple-darwin/bin` 放在 PATH 首位，实际 `rustc` 为 1.95.0、Clippy 为 0.1.95。仅 `rustup run 1.95 cargo clippy` 在此环境仍会找到 Homebrew 的 `cargo-clippy`，不能用于证明 Clippy 版本。

前端仍使用 Node 24.15.0 / pnpm 10.12.3；未把本轮结果记作 Node 22.12.0 验证。真实 Agent、桌面打包、双应用并存、nextest 全量未运行。

| 检查 | 结果 |
| --- | --- |
| Rust 1.95 `cargo test --lib --test golden --test provider_commands --test provider_service` | lib 3805 通过、0 失败、9 忽略；golden 43、provider_commands 12、provider_service 51 全部通过，共 106 项集成测试 |
| Rust 1.95 `cargo clippy -- -D warnings`、`cargo fmt --check` | 通过；移除遗留测试结构的未使用字段后，最终测试构建无该告警 |
| `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer` | 通过；renderer 保留原有构建警告 |
| `pnpm exec vitest run --maxWorkers=2 --minWorkers=2` | 最终全量 231 个文件、3030 项全部通过（120.49 秒）；首轮默认并发有 2 项 Pi 表单 5 秒超时，定向 48/48 通过后降低并发取得全量通过，未修改测试超时或 Pi 表单实现 |
| 文档本地链接 / `git diff --check` | 5 份本轮修改文档的相对文件目标存在；diff 空白检查通过 |

新增 2 项 Rust 单测、1 项后端集成测试、1 项前端组件测试：覆盖启动 catalog 初始化的数据库内容与源文件字节、显式导入可用性、新端口及既有端口保留、官方历史迁移缺少任一授权设置时保持文件不变，以及官方预设存在时手动导入按钮可达且渲染不导入。完整 Tauri setup、Pi 列表隐式同步、共享 Codex `custom` 路由、备份恢复冲突仍在 P0-02b2 范围。

本子步骤初轮 Rust 回归有 3 处默认端口断言仍期待 15721；核实实际输出为 18721 后仅修正这 3 处预期，显式传入旧端口的 fixture 继续保留。新的数据库回归确认已保存的 15721 不被自动改写。

## P0-02b2.1 验证（2026-10-20）

范围为 Codex 路由命名空间、外来表保留、官方历史迁移/恢复归属检查及对应回归；未修改前端运行时代码或新增依赖。沿用本机 Rust 1.95 工具链，完整 bin 路径置于 PATH 首位。基线为 `6f55f5ea`，下列结果针对本轮未提交工作区。

| 检查 | 结果 |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --test golden --test provider_commands --test provider_service` | lib 3808 通过、0 失败、9 忽略；golden 45、provider_commands 12 通过；provider_service 首次到达该套件时 50 通过、1 个旧清理语义断言失败，修复过程见下文 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test provider_service` | 修正测试后 51/51 通过；最终三个集成套件共 108 项通过 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Rust/Clippy 1.95 下通过，无 warning |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` / `git diff --check` | 通过 |
| 文档本地链接、需求原文与许可证 | 本轮 3 份修改文档的相对文件目标已核对；原始项目总结、LICENSE 与 HEAD 一致 |
| 前端 / 桌面 / 全量 nextest | 本轮未重跑前端测试与构建；前端未变。未运行桌面应用、真实 Agent、双应用并存 smoke 或全量 nextest，不能记作 V14 完成 |

新增 3 个库单测和 2 个 golden 集成测试，既有测试同步迁移路由预期：

- 外来 `custom` 表的普通/inline 写法、真实测试凭据、上游/Shield 占位符、profile 引用，在 6 种路由写入分支保持原值。
- 手动导入的表即使与数据库 ID/URL 相同，也不获得删除授权；通过公开切换入口检查表和 profile 的原文保留。生效 profile 阻止切换时，配置、mtime 和数据库快照不变。
- 只有 Shield 自有 ID 且形态精确匹配的官方镜像可以剥离；存在 profile 引用时保留。外来 `custom` 官方形态也不能被识别为 Shield 镜像。
- 通过公开历史迁移/恢复入口检查 JSONL 和真实临时 SQLite：只迁移 `openai`；仅按目录、目标路由和会话账本恢复；外来 `custom` 文件字节及数据库字段始终不变。
- 同目录旧 `custom` 完成标记不会阻止新的专属桶迁移；无元数据、损坏元数据、缺失目标和旧目标备份均不能用于恢复。历史门控覆盖生效 profile 覆盖顶层选路及缺失 profile 的负例。

调试记录：首轮库测试有 38 个旧路由/清理语义 fixture 或断言失配，适配后全部通过。golden 随后发现一处 deeplink 快照期待 `custom`，已核对仅更新 selector 和表头；另一处新测试要求两个外来段落保持相邻，但插入 Shield 表会改变相邻关系，改为逐段检查原文（含注释/引号）和 profile 保留。最终 provider_service 的旧断言要求清除任意表中的代理占位符，现改为检查活跃 Shield 路由使用新地址/Key、外来表完整保留、占位符不回填数据库；定向重跑 51/51 通过。未放宽生产检查或批量接受未知快照。

兼容性与未验收范围：旧 `custom` 历史和旧账本保留，不自动改写或纳入新账本；新路由 ID 的任意外部预占处理、完整首写备份/冲突恢复和真实并存属于 P0-02b2.3。本轮通过只证明上述自动化子集，不证明脱敏内核或端到端安全能力已实现。

## 1. 测试层次

1. 内核单元测试：规则、Unicode、重叠匹配、作用域和 token，优先 Rust 内置测试与现有测试依赖。
2. 存储与密钥集成：临时目录与测试密钥后端，平台钥匙串单独验证；不访问日常用户库。
3. 代理集成：本地 mock 上游捕获真实 HTTP 数据；每次发送和重试均计数，包含 reqwest/SOCKS 与 hyper 路径。
4. Agent E2E：隔离账户/配置，合成秘密，记录 Agent 版本、上游协议、平台及配置快照。
5. UI 验证：状态与错误文案对应真实后端状态；测试恢复值不进入事件或前端数据缓存。

## 2. 安全验收矩阵

| ID | 场景 | 可观察的通过条件 |
| --- | --- | --- |
| V01 | 单值、多值、重复、重叠、中文、Unicode、JSON 转义 | 上游捕获内容没有合成秘密；恢复后目标字段符合期望，结构字段不变 |
| V02 | system/历史/工具结果/工具参数/metadata、未知字段、超大 body | 声明支持字段全覆盖；未知高风险字段和超限内容阻断，实际发送数为 0 |
| V03 | 转换、整流重试、超时重试、备用 Provider、不同传输 | 每次 attempt 均受保护；映射集合一致；失败不会改用原文重发 |
| V04 | 并发、跨项目、伪造 session、未知 token、碰撞、到期与重启 | 未授权恢复次数为 0；合法范围正确恢复；重试和多轮引用符合授权契约 |
| V05 | 库/WAL/journal/备份/临时文件/日志/崩溃信息/导出 | 允许的测试源和本地接收结果之外，无合成秘密明文；篡改密文拒绝解密 |
| V06 | token 全切点、UTF-8 跨片、SSE event 切分、多输出块、取消/截断 | 正例与非流式结果一致；不跨块拼接；失败可观测，内存符合冻结上限 |
| V07 | 无密钥、钥匙串锁定、策略损坏、检测异常、磁盘满/写失败 | 请求前故障时上游请求数为 0；在途故障中止交付且不触发未保护重试 |
| V08 | 停止/kill 代理、重启、升级、配置外部修改、模式切换 | 受管 Agent 请求失败且无直连；恢复不覆盖外部修改；关闭保护须显式操作 |
| V09 | header/query/URL 秘密、302/307/308、目标变化、认证转发 | 非批准目标不接收请求；不自动跟随跳转；只向批准目标发送对应认证凭据 |
| V10 | 非绑定客户端、浏览器 Origin、未认证调用、越权 IPC | 不可触发受保护发送或获得原值；本地接口不暴露映射查询能力 |
| V11 | 图片、文件 URL/base64、音频、签名/加密块、未知编码 | 未支持内容明确阻断，不静默剥离或透明透传 |
| V12 | 模型把 token 放进外网工具参数、错误消息或多个工具块 | 未批准工具恢复被阻断；错误/审计无原值；合法普通文本按授权恢复 |
| V13 | 子 Agent、后台任务、独立环境变量、工具外网请求、手动直连 | 报告每条路径为已覆盖/阻断/未覆盖；观察到绕过不得记为已阻断 |
| V14 | CC Switch 与 Shield 并存、更新检查、目录和协议关联 | 应用身份、数据、scheme 独立；不读写旧库，不请求旧更新地址、不安装旧更新 |

所有负例都检查实际发送结果，不能只断言返回了某个错误码。跨会话错误恢复、故障放行或磁盘明文出现任一实例，就阻止对应范围验收。

## 3. 数据与指标

测试秘密使用专属合成 canary 字符串；上游 mock 保留接收到的字节供测试断言，生产日志仍不得存正文。扫描时明确白名单为 fixture 源文件和指定本地接收产物，不能用全局排除日志目录掩盖泄漏。

| 指标 | 计算口径 |
| --- | --- |
| 检测精确率 / 召回率 | TP/(TP+FP)、TP/(TP+FN)，给出标注规则与各类样本数 |
| 脱敏成功率 | 已标注应替换项中，实际出站符合策略的项数 / 应替换项数 |
| 恢复正确率 | 授权且应恢复的 token 中，准确恢复数 / 应恢复数 |
| 错误恢复 | 未授权/错误恢复事件数及全部负例数；验收要求已定义负例为 0 次 |
| 故障阻断率 | 受控路径故障场景中未发生未保护发送的数量 / 故障场景数 |
| 旁路泄漏 | 各条旁路有无 canary 到达观测上游，分别报告，不与已保护路径合并稀释 |
| 性能 | 同机同输入，对比原代理与启用 Shield 的附加 p50/p95、首字延迟、吞吐、峰值内存 |

分母为 0 时标记“不适用”，不记作 100%。有限样本中的 0 次失败不证明全局零风险。P0 冻结输入大小、并发、token 数量、缓冲/超时和性能预算，P4 用固定环境复测。

## 4. 检查命令与证据模板

现有命令可直接作为工程回归入口：

```sh
pnpm typecheck
pnpm format:check
pnpm test:unit
pnpm build:renderer
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci
```

Rust 检查需安装锁定工具链、平台依赖及 cargo-nextest；普通 `cargo test` 可用于定向开发验证，但不能把有不同隔离语义的运行结果冒充 CI nextest 结果。安全模块新增后提供明确测试过滤器或脚本，当前不伪造不存在的测试命令。

每份验收报告至少包含：commit、时间与时区、OS、工具链、Agent 版本、模型/协议、配置与策略版本、Vxx 用例、样本数、期望/实测、捕获与日志扫描结果、失败项、跳过原因、残余风险和复现命令。原始证据只用合成数据，密钥不进入报告。
