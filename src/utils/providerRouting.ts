import type { Provider } from "@/types";

/**
 * 供应商是否支持经本地代理路由（含模型层级路由）。
 *
 * 官方供应商（category === "official"）不可路由：无可劫持的 base_url、
 * 认证为 1P OAuth/订阅（代理无法注入凭据）。其余分类（含 cn_official 等
 * 自带 base_url + API key 的官方）均可经代理转发。
 *
 * 这是「是否支持路由」的单一真相源——ProviderCard 的「不支持路由」徽章、
 * 接管切换拦截、模型层级路由下拉过滤都应调用本函数。后续若出现新的不可
 * 路由分类（包括非官方的），只在此一处扩展，避免判据在各处漂移。
 */
export function supportsRouting(provider: Provider): boolean {
  return provider.category !== "official";
}
