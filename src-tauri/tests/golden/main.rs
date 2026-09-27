//! 重构前的金标测试。
//!
//! 接下来的重构会把 Claude Code / Codex / Gemini / Grok 的切换从「整份覆盖 live」改成
//! 「只替换关键字段」，并把本地路由改成显式的直连 / 代理两种模式。重构的前提是行为不变，
//! 所以这里先在旧代码上锁住需要保留的行为，新代码必须同样通过：
//!
//! - ① 按字节锁（快照在 `snapshots/`）：和被改文件同处一地、重构不该碰的内容，
//!   比如 MCP 投影的输出、深链导入和首启导入写出的 DB 原始行、live 文件的路径与权限位。
//! - ② 按字段锁（普通断言）：Claude 关键字段的切换结果、Codex 的安全红线。
//! - ③ 被替代的行为（整份覆盖、回填、片段合并、备份恢复）只在重构方案里记录，不在这里锁。
//!
//! 降级的旧版一侧（旧版按 `PROXY_MANAGED` 字面值识别接管态、没有备份行时按当前供应商
//! 重建 live）随接管备份机制一起退役，发版时改用上一个正式版的二进制做降级回放。新版
//! 这一侧由 `mode::controller` 的测试锁住：代理契约沿用这个字面值，启动时删掉旧备份行。
//!
//! 更新快照：`CC_SWITCH_UPDATE_GOLDEN=1 cargo test --test golden`，再逐个审 diff。
//! 快照变了就意味着行为变了，要能说清楚为什么。

#[allow(dead_code)]
#[path = "../support.rs"]
mod support;

mod claude_key_fields;
mod codex_red_lines;
mod file_modes;
mod import_rows;
mod mcp_bytes;
mod util;
