# OMP Vibe 模板：本地原型

本目录不接入 CC Switch 主工程，不改变现有的 OMP 供应商行为。原型复用项目内已有的 React 组件、视觉 token 和依赖，无需为它另装包。

## 运行

从仓库根目录启动（需要 Node.js 和已安装的仓库前端依赖）：

```sh
node local-omp-vibe-profiles/serve.mjs
```

打开终端显示的本地 URL。默认用临时目录和三套示例（智谱、GPT、智谱 + GPT）；退出后预览数据不保留。示例模型仅用于展示，不代表账户已授权。

预览确认后，可显式指定真实 OMP 目录：

```sh
node local-omp-vibe-profiles/serve.mjs --live
node local-omp-vibe-profiles/serve.mjs --agent-dir "<OMP agent 目录>"
```

`--live` 用 `omp config path` 解析当前 profile；如果 OMP 配置了独立目录、通过 `--profile` 启动或 CC Switch 指定了目录，用 `--agent-dir` 指向对应的全局 agent 目录。关闭本地服务不会恢复已经应用的模板。

## 行为

- 打开对话框先回显当前全局 Worker、默认模型和已发现的角色。载入「当前配置」会带入两个 Worker 值；其他角色默认不纳入，勾选后可按当前值另存或改选。保存、更新、删除模板不会改 OMP 的运行配置。
- 刷新时从全局 `config.yml` 的 `modelRoles`、`modelTags`、`cycleOrder` 和已存模板发现角色，不用随 OMP 更新维护一份角色表。已核对的 OMP CLI 不公开完整的未配置内置角色清单；配置中尚未出现的角色可以在「添加角色」里输入 `@角色名`，无需改代码。这里仍从模型目录选模型，不手填模型 ID。
- 「应用模板」只改 `task.agentModelOverrides.sonic`、`.task` 和勾选纳入的 `modelRoles` 条目。未纳入或后来新增的角色原样保留，`modelRoles.default` 始终只读。「清除显式设置」是删除已纳入角色的全局绑定，不等于取消勾选。模型目录、供应商配置和登录数据不写入。
- 两个 Worker override 也可能作用于同名普通子代理；全局角色绑定也不专属于 `/vibe`。项目配置和启动参数的优先级仍由 OMP 决定，已有 Worker 不会被重启。
- 命名模板与恢复值存于 agent 目录的 `.cc-switch-vibe-profiles.json`。两个 Worker 保存首次写入前的值；单个角色在首次被模板实际改动时记录原值。「恢复原设置」只还原这些值，不回滚其他配置。
- 检测到外部修改会拒绝写入并提示刷新。目标 YAML 结构如果无法安全定位（例如非空行内映射、目标别名），维持原文件不动。真实模式使用 `omp models --json --no-extensions` 加载聊天模型；失败时可刷新，且不影响读取或应用已保存的模板。目录中出现的模型不代表账号套餐已授权。

## 嵌入 CC Switch

在 `src/App.tsx` 的 `renderAppPageHeader()` 中，把 `OmpVibeProfilesAction` 放在 `AppPageHeader.actions` 的搜索按钮前，仅当 `activeApp === "omp" && currentView === "providers"` 时渲染。入口是 `quiet` 次要按钮，现有「添加供应商」继续是该页唯一实心主操作；不新建侧栏、页签或供应商卡。弹窗复用项目的 `Dialog`、`Button`、`Select` 和表单样式，关闭后回到原供应商页。

正式接入时，将 `VibeProfilesClient` 从预览 HTTP 适配器换成 Tauri 命令，后端复用 `omp_config::get_omp_agent_dir()` 和原子写入约定，把文案纳入八套 i18n。不要把 Vite 本地服务作为生产依赖，也不要把此目录直接作为 PR 内容。

## 上游流程

已有 [CC Switch #9502](https://github.com/farion1231/cc-switch/issues/9502)，维护者请先不要开 PR。确认本地效果后，优先在原 Issue 讨论：保存混用方案、动态发现 OMP 角色、只写模板明确纳入的角色、保留默认模型。等维护者认可后，再从最新 `main` 开单独分支做正式接入并关联该 Issue；不修改 `SCHEMA_VERSION`。

若维护者要求另开 Issue，用 [Feature Request 表单](https://github.com/farion1231/cc-switch/issues/new?template=feature_request.yml)，`Related App` 若还没有 OMP 就选 `General / 通用`，并关联 #9502。Issue 和 PR 的 **Human Note / 本人说明须由你亲自写，不允许 AI 生成**。PR 还需补关联 Issue、截图和真实验证结果；详见仓库 `CONTRIBUTING.md` 和 PR 模板。

原生依据：[OMP 19.1.0 / Vibe](https://github.com/can1357/oh-my-pi/blob/v19.1.0/docs/vibe-mode.md#the-two-worker-tiers)、[Worker 路由](https://github.com/can1357/oh-my-pi/blob/v19.1.0/docs/task-agent-discovery.md#vibe_spawn-tier-routing)。AI Toolbox 仅用于比较多方案交互，不复用其应用语义或代码。
