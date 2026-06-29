import type { CustomEndpoint, ProviderMeta } from "@/types";

/**
 * 合并供应商元数据中的自定义端点。
 * - 当 customEndpoints 为空对象时，明确删除自定义端点但保留其它元数据。
 * - 当 customEndpoints 为 null/undefined 时，不修改端点（保留原有端点）。
 * - 当 customEndpoints 存在时，覆盖原有自定义端点。
 * - 若结果为空对象且非明确清空场景则返回 undefined，避免写入空 meta。
 */
export function mergeProviderMeta(
  initialMeta: ProviderMeta | undefined,
  customEndpoints: Record<string, CustomEndpoint> | null | undefined,
): ProviderMeta | undefined {
  const hasCustomEndpoints =
    !!customEndpoints && Object.keys(customEndpoints).length > 0;

  // 明确清空：传入空对象（非 null/undefined）表示用户想要删除所有端点
  const isExplicitClear =
    customEndpoints !== null &&
    customEndpoints !== undefined &&
    Object.keys(customEndpoints).length === 0;

  if (hasCustomEndpoints) {
    return {
      ...(initialMeta ? { ...initialMeta } : {}),
      custom_endpoints: customEndpoints!,
    };
  }

  // 明确清空端点
  if (isExplicitClear) {
    if (!initialMeta) {
      // 新供应商且用户没有添加端点（理论上不会到这里）
      return undefined;
    }

    if ("custom_endpoints" in initialMeta) {
      const { custom_endpoints, ...rest } = initialMeta;
      // 保留其他字段（如 usage_script）
      // 即使 rest 为空，也要返回空对象（让后端知道要清空 meta）
      return Object.keys(rest).length > 0 ? rest : {};
    }

    // initialMeta 中本来就没有 custom_endpoints
    return { ...initialMeta };
  }

  // null/undefined：用户没有修改端点，保持不变
  if (!initialMeta) {
    return undefined;
  }

  if ("custom_endpoints" in initialMeta) {
    const { custom_endpoints, ...rest } = initialMeta;
    return Object.keys(rest).length > 0 ? rest : undefined;
  }

  return { ...initialMeta };
}
