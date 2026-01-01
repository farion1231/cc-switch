import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

// ===== 流式健康检查类型 =====

export type HealthStatus = "operational" | "degraded" | "failed";

export interface StreamCheckConfig {
  timeoutSecs: number;
  maxRetries: number;
  degradedThresholdMs: number;
  claudeModel: string;
  codexModel: string;
  geminiModel: string;
}

export interface StreamCheckResult {
  status: HealthStatus;
  success: boolean;
  message: string;
  responseTimeMs?: number;
  httpStatus?: number;
  modelUsed: string;
  testedAt: number;
  retryCount: number;
}

// ===== TPS 测试类型 =====

export type TokenSource = "usage" | "estimated";

export interface TpsTestResult {
  success: boolean;
  message: string;
  modelUsed: string;
  httpStatus?: number;
  responseTimeMs: number;
  outputTokens?: number;
  tokensPerSecond?: number;
  tokenSource?: TokenSource;
  testedAt: number;
}

// ===== 流式健康检查 API =====

/**
 * 流式健康检查（单个供应商）
 */
export async function streamCheckProvider(
  appType: AppId,
  providerId: string,
): Promise<StreamCheckResult> {
  return invoke("stream_check_provider", { appType, providerId });
}

/**
 * 批量流式健康检查
 */
export async function streamCheckAllProviders(
  appType: AppId,
  proxyTargetsOnly: boolean = false,
): Promise<Array<[string, StreamCheckResult]>> {
  return invoke("stream_check_all_providers", { appType, proxyTargetsOnly });
}

/**
 * 获取流式检查配置
 */
export async function getStreamCheckConfig(): Promise<StreamCheckConfig> {
  return invoke("get_stream_check_config");
}

/**
 * 保存流式检查配置
 */
export async function saveStreamCheckConfig(
  config: StreamCheckConfig,
): Promise<void> {
  return invoke("save_stream_check_config", { config });
}

// ===== TPS 测试 API =====

/**
 * TPS 测试（单个供应商）
 */
export async function tpsTestProvider(
  appType: AppId,
  providerId: string,
): Promise<TpsTestResult> {
  return invoke("tps_test_provider", { appType, providerId });
}
