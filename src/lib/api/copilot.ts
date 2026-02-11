/**
 * GitHub Copilot OAuth API
 *
 * 提供 GitHub Copilot OAuth 设备码流程相关的 API 函数。
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * GitHub 设备码响应
 */
export interface CopilotDeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

/**
 * Copilot 认证状态
 */
export interface CopilotAuthStatus {
  authenticated: boolean;
  username: string | null;
  expires_at: number | null;
}

/**
 * 启动 GitHub OAuth 设备码流程
 *
 * @returns 设备码响应，包含用户码和验证 URL
 */
export async function copilotStartDeviceFlow(): Promise<CopilotDeviceCodeResponse> {
  return invoke<CopilotDeviceCodeResponse>("copilot_start_device_flow");
}

/**
 * 轮询 OAuth Token
 *
 * 使用设备码轮询 GitHub，等待用户完成授权。
 *
 * @param deviceCode - 设备码
 * @returns true 表示认证成功，false 表示仍在等待用户授权
 */
export async function copilotPollForAuth(deviceCode: string): Promise<boolean> {
  return invoke<boolean>("copilot_poll_for_auth", {
    deviceCode,
  });
}

/**
 * 获取 Copilot 认证状态
 *
 * @returns 认证状态，包含是否已认证、用户名和过期时间
 */
export async function copilotGetAuthStatus(): Promise<CopilotAuthStatus> {
  return invoke<CopilotAuthStatus>("copilot_get_auth_status");
}

/**
 * 注销 Copilot 认证
 */
export async function copilotLogout(): Promise<void> {
  return invoke("copilot_logout");
}

/**
 * 检查是否已认证
 *
 * @returns true 表示已认证
 */
export async function copilotIsAuthenticated(): Promise<boolean> {
  return invoke<boolean>("copilot_is_authenticated");
}

/**
 * Copilot 可用模型
 */
export interface CopilotModel {
  id: string;
  name: string;
  vendor: string;
  model_picker_enabled: boolean;
}

/**
 * 获取有效的 Copilot Token
 *
 * 内部使用，用于代理请求。
 *
 * @returns Copilot Token
 */
export async function copilotGetToken(): Promise<string> {
  return invoke<string>("copilot_get_token");
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
