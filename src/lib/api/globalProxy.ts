/**
 * 全局出站代理 API
 *
 * 提供获取、设置和测试全局代理的功能。
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * 代理测试结果
 */
export interface ProxyTestResult {
  success: boolean;
  latencyMs: number;
  error: string | null;
}

/**
 * 出站代理状态
 */
export interface UpstreamProxyStatus {
  enabled: boolean;
  proxyUrl: string | null;
}

/**
 * 检测到的代理
 */
export interface DetectedProxy {
  url: string;
  proxyType: string;
  port: number;
}

/**
 * 获取全局代理 URL
 *
 * @returns 代理 URL，null 表示未配置（直连）
 */
export async function getGlobalProxyUrl(): Promise<string | null> {
  return invoke<string | null>("get_global_proxy_url");
}

/**
 * 设置全局代理 URL
 *
 * @param url - 代理 URL（如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080）
 *              空字符串表示清除代理（直连）
 */
export async function setGlobalProxyUrl(url: string): Promise<void> {
  try {
    return await invoke("set_global_proxy_url", { url });
  } catch (error) {
    // Tauri invoke 错误可能是字符串
    throw new Error(typeof error === "string" ? error : String(error));
  }
}

/**
 * 测试代理连接
 *
 * @param url - 要测试的代理 URL
 * @returns 测试结果，包含是否成功、延迟和错误信息
 */
export async function testProxyUrl(url: string): Promise<ProxyTestResult> {
  return invoke<ProxyTestResult>("test_proxy_url", { url });
}

/**
 * 获取当前出站代理状态
 *
 * @returns 代理状态，包含是否启用和代理 URL
 */
export async function getUpstreamProxyStatus(): Promise<UpstreamProxyStatus> {
  return invoke<UpstreamProxyStatus>("get_upstream_proxy_status");
}

/**
 * 扫描本地代理
 *
 * @returns 检测到的代理列表
 */
export async function scanLocalProxies(): Promise<DetectedProxy[]> {
  return invoke<DetectedProxy[]>("scan_local_proxies");
}
