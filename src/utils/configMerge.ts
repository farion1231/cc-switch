/**
 * 配置合并工具函数
 *
 * 用于公共配置重构后的运行时合并逻辑：
 * - computeFinalConfig: 计算最终配置（通用配置 + 自定义配置）
 * - extractDifference: 从 live 配置中提取与通用配置不同的部分
 */

// ============================================================================
// 工具函数
// ============================================================================

/**
 * 检查值是否为普通对象
 */
export const isPlainObject = (
  value: unknown,
): value is Record<string, unknown> => {
  return Object.prototype.toString.call(value) === "[object Object]";
};

/**
 * 深拷贝对象
 */
export const deepClone = <T>(obj: T): T => {
  if (obj === null || typeof obj !== "object") return obj;
  if (obj instanceof Date) return new Date(obj.getTime()) as T;
  if (Array.isArray(obj)) return obj.map((item) => deepClone(item)) as T;
  if (isPlainObject(obj)) {
    const clonedObj = {} as Record<string, unknown>;
    for (const key in obj) {
      if (Object.prototype.hasOwnProperty.call(obj, key)) {
        clonedObj[key] = deepClone((obj as Record<string, unknown>)[key]);
      }
    }
    return clonedObj as T;
  }
  return obj;
};

/**
 * 深度相等比较
 */
export const deepEqual = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;

  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;

  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    return a.every((item, index) => deepEqual(item, b[index]));
  }

  if (isPlainObject(a) && isPlainObject(b)) {
    const keysA = Object.keys(a);
    const keysB = Object.keys(b);
    if (keysA.length !== keysB.length) return false;
    return keysA.every((key) => deepEqual(a[key], b[key]));
  }

  return false;
};

/**
 * 检查 source 是否是 target 的子集
 * 即 source 中的所有键值对都存在于 target 中
 */
export const isSubset = (target: unknown, source: unknown): boolean => {
  if (isPlainObject(source)) {
    if (!isPlainObject(target)) return false;
    return Object.entries(source).every(([key, value]) =>
      isSubset(target[key], value),
    );
  }

  if (Array.isArray(source)) {
    if (!Array.isArray(target) || target.length !== source.length) return false;
    return source.every((item, index) => isSubset(target[index], item));
  }

  return target === source;
};

// ============================================================================
// 配置合并函数
// ============================================================================

/**
 * 深度合并两个对象（source 覆盖 target）
 *
 * 合并规则：
 * - 嵌套对象：递归合并
 * - 数组：source 完全替换 target（不做元素级合并）
 * - 原始值：source 覆盖 target
 * - undefined/null：不覆盖（与后端 config_merge.rs 保持一致）
 */
export const deepMerge = <T extends Record<string, unknown>>(
  target: T,
  source: T,
): T => {
  const result = deepClone(target);

  for (const key of Object.keys(source)) {
    const sourceValue = source[key];
    const targetValue = result[key];

    // undefined 和 null 都不覆盖（与后端保持一致）
    if (sourceValue === undefined || sourceValue === null) {
      continue;
    }

    if (isPlainObject(sourceValue) && isPlainObject(targetValue)) {
      // 嵌套对象：递归合并
      result[key as keyof T] = deepMerge(
        targetValue as Record<string, unknown>,
        sourceValue as Record<string, unknown>,
      ) as T[keyof T];
    } else {
      // 其他情况（数组、原始值）：source 覆盖
      result[key as keyof T] = deepClone(sourceValue) as T[keyof T];
    }
  }

  return result;
};

/**
 * 计算最终配置
 *
 * 通用配置作为 base，自定义配置覆盖（自定义优先）
 *
 * @param customConfig - 供应商自定义配置（来自 settings_config 字段）
 * @param commonConfig - 通用配置片段（来自数据库）
 * @param enabled - 是否启用通用配置
 * @returns 合并后的最终配置
 */
export const computeFinalConfig = (
  customConfig: Record<string, unknown>,
  commonConfig: Record<string, unknown>,
  enabled: boolean,
): Record<string, unknown> => {
  const safeCustom = customConfig ?? {};
  const safeCommon = commonConfig ?? {};

  if (!enabled || Object.keys(safeCommon).length === 0) {
    return deepClone(safeCustom);
  }

  // 通用配置作为 base，自定义配置覆盖
  // 这样自定义配置的值会优先
  return deepMerge(safeCommon, safeCustom);
};

// ============================================================================
// 差异提取函数
// ============================================================================

/**
 * 差异提取结果
 */
export interface ExtractDifferenceResult {
  /** 自定义配置（与通用配置不同的部分） */
  customConfig: Record<string, unknown>;
  /** 是否检测到通用配置的键（用于判断是否应启用通用配置） */
  hasCommonKeys: boolean;
}

/**
 * 从 live 配置中提取与通用配置不同的部分作为自定义配置
 *
 * 提取规则：
 * - 通用配置中不存在的键 → 加入自定义配置
 * - 通用配置中存在但值不同 → 加入自定义配置（用户覆盖）
 * - 通用配置中存在且值相同 → 跳过（避免冗余存储）
 *
 * @param liveConfig - 从本地文件读取的配置
 * @param commonConfig - 通用配置片段
 * @returns { customConfig, hasCommonKeys }
 */
export const extractDifference = (
  liveConfig: Record<string, unknown>,
  commonConfig: Record<string, unknown>,
): ExtractDifferenceResult => {
  const customConfig: Record<string, unknown> = {};
  let hasCommonKeys = false;

  /**
   * 递归提取差异
   */
  const extract = (
    live: Record<string, unknown>,
    common: Record<string, unknown>,
    target: Record<string, unknown>,
  ): void => {
    for (const [key, liveValue] of Object.entries(live)) {
      const commonValue = common[key];

      if (commonValue === undefined) {
        // Case 1: 通用配置中不存在该键，完整保留到自定义配置
        target[key] = deepClone(liveValue);
      } else if (isPlainObject(liveValue) && isPlainObject(commonValue)) {
        // Case 2: 嵌套对象，递归处理
        const nested: Record<string, unknown> = {};
        extract(
          liveValue as Record<string, unknown>,
          commonValue as Record<string, unknown>,
          nested,
        );
        if (Object.keys(nested).length > 0) {
          // 嵌套对象有差异，保留差异部分
          target[key] = nested;
        } else {
          // 嵌套对象完全相同，标记有通用配置的键
          hasCommonKeys = true;
        }
      } else if (!deepEqual(liveValue, commonValue)) {
        // Case 3: 值不同，保留到自定义配置（用户覆盖）
        target[key] = deepClone(liveValue);
      } else {
        // Case 4: 值完全相同，不保存到自定义配置（避免冗余）
        hasCommonKeys = true;
      }
    }
  };

  extract(liveConfig, commonConfig, customConfig);

  return { customConfig, hasCommonKeys };
};
