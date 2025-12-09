import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

export interface ModelTestConfig {
  claudeModel: string;
  codexModel: string;
  geminiModel: string;
  testPrompt: string;
  timeoutSecs: number;
}

export interface ModelTestResult {
  success: boolean;
  message: string;
  responseTimeMs?: number;
  httpStatus?: number;
  modelUsed: string;
  testedAt: number;
}

export interface ModelTestLog {
  id: number;
  providerId: string;
  providerName: string;
  appType: string;
  model: string;
  prompt: string;
  success: boolean;
  message: string;
  responseTimeMs?: number;
  httpStatus?: number;
  testedAt: number;
}

/**
 * 测试单个供应商的模型可用性
 */
export async function testProviderModel(
  appType: AppId,
  providerId: string,
): Promise<ModelTestResult> {
  return invoke("test_provider_model", { appType, providerId });
}

/**
 * 批量测试所有供应商
 */
export async function testAllProvidersModel(
  appType: AppId,
  proxyTargetsOnly: boolean = false,
): Promise<Array<[string, ModelTestResult]>> {
  return invoke("test_all_providers_model", { appType, proxyTargetsOnly });
}

/**
 * 获取模型测试配置
 */
export async function getModelTestConfig(): Promise<ModelTestConfig> {
  return invoke("get_model_test_config");
}

/**
 * 保存模型测试配置
 */
export async function saveModelTestConfig(
  config: ModelTestConfig,
): Promise<void> {
  return invoke("save_model_test_config", { config });
}

/**
 * 获取模型测试日志
 */
export async function getModelTestLogs(
  appType?: string,
  providerId?: string,
  limit?: number,
): Promise<ModelTestLog[]> {
  return invoke("get_model_test_logs", { appType, providerId, limit });
}

/**
 * 清理旧的测试日志
 */
export async function cleanupModelTestLogs(
  keepCount?: number,
): Promise<number> {
  return invoke("cleanup_model_test_logs", { keepCount });
}
