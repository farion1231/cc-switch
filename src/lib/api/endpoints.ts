import { invoke } from "@tauri-apps/api/core";
import type { Endpoint, EndpointsMap, EndpointReference } from "@/types";

/**
 * 全局端点 API（端点引用化）
 *
 * 端点是厂商连接信息的单一事实源：base_url / api_key 一处编辑，
 * 所有引用它的分组条目在保存与端点更新时自动物化生效。
 */
export const endpointsApi = {
  /** 全部端点 */
  async getAll(): Promise<EndpointsMap> {
    return await invoke("get_endpoints");
  },

  /** 单个端点 */
  async get(id: string): Promise<Endpoint | null> {
    return await invoke("get_endpoint", { id });
  },

  /** 新增或更新端点（级联重物化所有引用条目） */
  async save(endpoint: Endpoint): Promise<void> {
    return await invoke("save_endpoint", { endpoint });
  },

  /**
   * 删除端点。被条目引用时后端拒绝并返回引用列表；
   * 本地包装为抛错（message 为引用计数的可读描述），面板另行展示引用明细。
   */
  async delete(id: string): Promise<void> {
    // 后端返回 Result<Result<(), Vec<(app,id)>>, String>（serde 外部标签形态）
    const result = await invoke<{ Ok: null } | { Err: EndpointReference[] }>(
      "delete_endpoint",
      { id },
    );
    if ("Ok" in result) {
      return;
    }
    const refs = result.Err;
    throw Object.assign(new Error(`REFERENCED:${refs.length}`), { refs });
  },

  /** 引用某端点的条目列表 [(app_type, provider_id)] */
  async referencingProviders(id: string): Promise<EndpointReference[]> {
    return await invoke("endpoint_referencing_providers", { id });
  },
};
