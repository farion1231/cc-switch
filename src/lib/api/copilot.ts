/**
 * GitHub Copilot OAuth API
 *
 * 提供 GitHub Copilot OAuth 设备码流程相关的 API 函数。
 * 支持多账号管理。
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * GitHub 账号信息（公开信息）
 */
export interface GitHubAccount {
  /** GitHub 用户 ID（唯一标识） */
  id: string;
  /** GitHub 用户名 */
  login: string;
  /** 头像 URL */
  avatar_url: string | null;
  /** 认证时间戳（Unix 秒） */
  authenticated_at: number;
  /** GitHub 域名（github.com 或 GHES 域名） */
  github_domain: string;
}

/**
 * Copilot 可用模型
 */
export type CopilotReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "ultra";

export interface CopilotModel {
  id: string;
  name: string;
  vendor: string;
  model_picker_enabled: boolean;
  context_window?: number;
  supported_endpoints?: string[];
  supports_parallel_tool_calls?: boolean;
  supports_vision?: boolean;
  reasoning_effort?: CopilotReasoningEffort[];
}

/**
 * 获取 Copilot 可用模型列表
 *
 * @returns 可用模型列表
 */
export async function copilotGetModels(): Promise<CopilotModel[]> {
  return invoke<CopilotModel[]>("copilot_get_models");
}

/**
 * 配额详情
 */
export interface QuotaDetail {
  entitlement: number;
  remaining: number;
  percent_remaining: number;
  unlimited: boolean;
  /** 当前计划是否含这一项（Free 的 premium_interactions 是 false）；旧响应没有 */
  has_quota?: boolean;
}

/**
 * 配额快照
 */
export interface QuotaSnapshots {
  chat: QuotaDetail;
  completions: QuotaDetail;
  premium_interactions: QuotaDetail;
}

/**
 * Copilot 使用量响应
 */
export interface CopilotUsageResponse {
  copilot_plan: string;
  /** 订阅 SKU（`free_limited_copilot` / `max_monthly_subscriber_quota` …），旧版后端没有 */
  access_type_sku?: string;
  quota_reset_date: string;
  quota_snapshots: QuotaSnapshots;
}

/**
 * 获取 Copilot 使用量信息
 *
 * @returns 使用量信息，包含计划类型、重置日期和配额快照
 */
export async function copilotGetUsage(): Promise<CopilotUsageResponse> {
  return invoke<CopilotUsageResponse>("copilot_get_usage");
}

/** Copilot CLI 当前登录的 GitHub 账号，以及授权中心里与它对应的账号。 */
export interface CopilotCliSignedInAccount {
  login: string;
  githubDomain: string;
  accountId: string | null;
}

/**
 * 读 Copilot CLI 当前登录的账号（`~/.copilot/config.json`），并在授权中心里按用户名和
 * 域名找对应账号。Copilot CLI 没登录时返回 `null`。
 */
export async function copilotCliSignedInAccount(): Promise<CopilotCliSignedInAccount | null> {
  return invoke<CopilotCliSignedInAccount | null>(
    "copilot_cli_signed_in_account",
  );
}

// ==================== 多账号管理 API ====================

/**
 * 获取指定账号的 Copilot 可用模型列表
 *
 * @param accountId - GitHub 用户 ID
 * @returns 可用模型列表
 */
export async function copilotGetModelsForAccount(
  accountId: string,
): Promise<CopilotModel[]> {
  return invoke<CopilotModel[]>("copilot_get_models_for_account", {
    accountId,
  });
}

/**
 * 获取指定账号的 Copilot 使用量信息
 *
 * @param accountId - GitHub 用户 ID
 * @returns 使用量信息
 */
export async function copilotGetUsageForAccount(
  accountId: string,
): Promise<CopilotUsageResponse> {
  return invoke<CopilotUsageResponse>("copilot_get_usage_for_account", {
    accountId,
  });
}
