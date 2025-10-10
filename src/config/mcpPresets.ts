import { McpServer } from "../types";

export type McpPreset = {
  id: string;
  name: string;
  description: string;
  tags?: string[];
  server: McpServer;
  homepage?: string;
  docs?: string;
  requiresEnv?: string[];
};

// 预设库（数据文件，当前未接入 UI，便于后续“一键启用”）
// 注意：预设数据暂时清空，仅保留结构与引用位置。
// 原因：
// - 近期决定将 MCP SSOT 拆分为 mcp.claude / mcp.codex，不同客户端的格式与支持能力不同；
// - 需要先完善“隐藏预设/不回种”机制与导入/同步策略，避免用户删除后被自动回填；
// - 在上述机制与 Codex 适配完成前，避免内置示例误导或造成意外写入。
// 后续计划（占位备注）：
// - 重新引入官方/社区 MCP 预设，区分 `source: "preset"`；
// - 支持每客户端（Claude/Codex）独立隐藏名单 `hiddenPresets`，仅影响自动回种；
// - UI 提供“删除并隐藏”与“恢复预设”操作；
// - 导入/同步与启用状态解耦，仅启用项投影至对应客户端的用户配置文件。
export const mcpPresets: McpPreset[] = [];

export default mcpPresets;
