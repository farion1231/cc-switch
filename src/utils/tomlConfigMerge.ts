/**
 * TOML 配置合并工具函数
 *
 * 用于 Codex 的 TOML 格式配置合并和差异提取。
 * TOML 配置需要先解析为对象，然后使用通用的合并/提取算法。
 */

import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import { normalizeTomlText } from "@/utils/textNormalization";
import {
  computeFinalConfig,
  extractDifference,
  isPlainObject,
} from "./configMerge";

// ============================================================================
// TOML 解析/序列化工具
// ============================================================================

/**
 * 安全解析 TOML 字符串为对象
 */
export const safeParseToml = (
  tomlString: string,
): { config: Record<string, unknown> | null; error: string | null } => {
  try {
    if (!tomlString.trim()) {
      return { config: {}, error: null };
    }
    const normalized = normalizeTomlText(tomlString);
    const parsed = parseToml(normalized);
    if (!isPlainObject(parsed)) {
      return { config: null, error: "TOML 解析结果不是对象" };
    }
    return { config: parsed as Record<string, unknown>, error: null };
  } catch (e) {
    return {
      config: null,
      error: e instanceof Error ? e.message : "TOML 解析失败",
    };
  }
};

/**
 * 将对象序列化为 TOML 字符串
 * 并移除冗余的空父级 section 头（如 [model_providers] 后面紧跟 [model_providers.packycode]）
 */
export const safeStringifyToml = (
  config: Record<string, unknown>,
): { toml: string; error: string | null } => {
  try {
    if (Object.keys(config).length === 0) {
      return { toml: "", error: null };
    }
    let toml = stringifyToml(config);

    // 移除冗余的空父级 section 头
    // 例如：[model_providers]\n[model_providers.packycode] -> [model_providers.packycode]
    toml = removeEmptyParentSections(toml);

    return { toml, error: null };
  } catch (e) {
    return {
      toml: "",
      error: e instanceof Error ? e.message : "TOML 序列化失败",
    };
  }
};

/**
 * 移除冗余的空父级 section 头
 * 当一个 section 头后面紧跟的是它的子 section（没有任何键值对），则移除这个空的父级 section
 */
function removeEmptyParentSections(toml: string): string {
  const lines = toml.split("\n");
  const result: string[] = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();

    // 检查是否是 section 头
    const sectionMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      const currentSection = sectionMatch[1];

      // 查找下一个非空行
      let nextIdx = i + 1;
      while (nextIdx < lines.length && !lines[nextIdx].trim()) {
        nextIdx++;
      }

      // 如果下一个非空行也是一个 section 头，检查是否是子 section
      if (nextIdx < lines.length) {
        const nextLine = lines[nextIdx].trim();
        const nextSectionMatch = nextLine.match(/^\[([^\]]+)\]$/);
        if (nextSectionMatch) {
          const nextSection = nextSectionMatch[1];
          // 如果下一个 section 是当前 section 的子 section，跳过当前空的父 section
          if (nextSection.startsWith(currentSection + ".")) {
            // 跳过当前行和之间的空行
            continue;
          }
        }
      }
    }

    result.push(line);
  }

  return result.join("\n");
}

// ============================================================================
// 键顺序重排工具
// ============================================================================

/**
 * 重排合并后的对象键顺序：自定义配置的键在前，通用配置独有的键在后
 *
 * 这确保序列化后的 TOML 中用户自定义的键（如 model_provider, model）显示在顶部，
 * 而通用配置独有的键（如 model_reasoning_effort）显示在下面。
 *
 * @param merged - 合并后的配置对象
 * @param custom - 自定义配置对象
 * @param common - 通用配置对象
 * @returns 重排键顺序后的配置对象
 */
function reorderMergedKeys(
  merged: Record<string, unknown>,
  custom: Record<string, unknown>,
  common: Record<string, unknown>,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};

  // 1. 先添加自定义配置中的键（保持自定义配置中的顺序）
  for (const key of Object.keys(custom)) {
    if (key in merged) {
      const mergedValue = merged[key];
      const customValue = custom[key];
      const commonValue = common[key];

      // 如果是嵌套对象，递归重排
      if (
        isPlainObject(mergedValue) &&
        isPlainObject(customValue) &&
        isPlainObject(commonValue)
      ) {
        result[key] = reorderMergedKeys(
          mergedValue as Record<string, unknown>,
          customValue as Record<string, unknown>,
          commonValue as Record<string, unknown>,
        );
      } else if (isPlainObject(mergedValue) && isPlainObject(customValue)) {
        // 通用配置中没有这个嵌套对象，直接使用合并后的值
        result[key] = mergedValue;
      } else {
        result[key] = mergedValue;
      }
    }
  }

  // 2. 再添加通用配置独有的键（不在自定义配置中）
  for (const key of Object.keys(common)) {
    if (!(key in custom) && key in merged) {
      result[key] = merged[key];
    }
  }

  return result;
}

// ============================================================================
// TOML 配置合并函数
// ============================================================================

/**
 * 计算最终 TOML 配置
 *
 * @param customToml - 自定义 TOML 配置字符串
 * @param commonToml - 通用 TOML 配置字符串
 * @param enabled - 是否启用通用配置
 * @returns 合并后的 TOML 字符串
 */
export const computeFinalTomlConfig = (
  customToml: string,
  commonToml: string,
  enabled: boolean,
): { finalConfig: string; error?: string } => {
  // 如果未启用或通用配置为空，直接返回自定义配置
  if (!enabled || !commonToml.trim()) {
    return { finalConfig: customToml };
  }

  // 解析自定义配置
  const customResult = safeParseToml(customToml);
  if (customResult.error) {
    return {
      finalConfig: customToml,
      error: `自定义配置解析失败: ${customResult.error}`,
    };
  }

  // 解析通用配置
  const commonResult = safeParseToml(commonToml);
  if (commonResult.error) {
    return {
      finalConfig: customToml,
      error: `通用配置解析失败: ${commonResult.error}`,
    };
  }

  // 使用通用合并函数
  const merged = computeFinalConfig(
    customResult.config!,
    commonResult.config!,
    true, // enabled 已在上面检查
  );

  // 重排键顺序：自定义配置的键在前，通用配置独有的键在后
  // 这样序列化后 TOML 输出中用户自定义的键（如 model_provider, model）会在顶部
  const reordered = reorderMergedKeys(
    merged,
    customResult.config!,
    commonResult.config!,
  );

  // 序列化回 TOML
  const stringifyResult = safeStringifyToml(reordered);
  if (stringifyResult.error) {
    return {
      finalConfig: customToml,
      error: `TOML 序列化失败: ${stringifyResult.error}`,
    };
  }

  return { finalConfig: stringifyResult.toml };
};

// ============================================================================
// TOML 差异提取函数
// ============================================================================

/**
 * TOML 差异提取结果
 */
export interface ExtractTomlDifferenceResult {
  /** 自定义 TOML 配置字符串（与通用配置不同的部分） */
  customToml: string;
  /** 是否检测到通用配置的键 */
  hasCommonKeys: boolean;
  /** 错误信息 */
  error?: string;
}

/**
 * 从 live TOML 配置中提取与通用配置不同的部分
 *
 * @param liveToml - 从本地文件读取的 TOML 字符串
 * @param commonToml - 通用 TOML 配置字符串
 * @returns { customToml, hasCommonKeys, error }
 */
export const extractTomlDifference = (
  liveToml: string,
  commonToml: string,
): ExtractTomlDifferenceResult => {
  // 如果通用配置为空，live 配置就是自定义配置
  if (!commonToml.trim()) {
    return { customToml: liveToml, hasCommonKeys: false };
  }

  // 解析 live 配置
  const liveResult = safeParseToml(liveToml);
  if (liveResult.error) {
    return {
      customToml: liveToml,
      hasCommonKeys: false,
      error: `Live 配置解析失败: ${liveResult.error}`,
    };
  }

  // 解析通用配置
  const commonResult = safeParseToml(commonToml);
  if (commonResult.error) {
    return {
      customToml: liveToml,
      hasCommonKeys: false,
      error: `通用配置解析失败: ${commonResult.error}`,
    };
  }

  // 使用通用差异提取函数
  const diffResult = extractDifference(
    liveResult.config!,
    commonResult.config!,
  );

  // 序列化回 TOML
  const stringifyResult = safeStringifyToml(diffResult.customConfig);
  if (stringifyResult.error) {
    return {
      customToml: liveToml,
      hasCommonKeys: false,
      error: `TOML 序列化失败: ${stringifyResult.error}`,
    };
  }

  return {
    customToml: stringifyResult.toml,
    hasCommonKeys: diffResult.hasCommonKeys,
  };
};

/**
 * 检查 TOML 配置是否包含通用配置的内容
 * 使用对象级别比较，而非字符串匹配
 */
export const hasTomlCommonConfig = (
  tomlString: string,
  commonTomlString: string,
): boolean => {
  if (!commonTomlString.trim()) return false;

  const liveResult = safeParseToml(tomlString);
  const commonResult = safeParseToml(commonTomlString);

  if (liveResult.error || commonResult.error) {
    return false;
  }

  // 检查 common 中的所有键是否存在于 live 中且值相等
  const checkSubset = (
    live: Record<string, unknown>,
    common: Record<string, unknown>,
  ): boolean => {
    for (const [key, commonValue] of Object.entries(common)) {
      const liveValue = live[key];

      if (liveValue === undefined) {
        return false;
      }

      if (isPlainObject(commonValue) && isPlainObject(liveValue)) {
        if (
          !checkSubset(
            liveValue as Record<string, unknown>,
            commonValue as Record<string, unknown>,
          )
        ) {
          return false;
        }
      } else if (Array.isArray(commonValue) && Array.isArray(liveValue)) {
        if (JSON.stringify(commonValue) !== JSON.stringify(liveValue)) {
          return false;
        }
      } else if (liveValue !== commonValue) {
        return false;
      }
    }
    return true;
  };

  return checkSubset(liveResult.config!, commonResult.config!);
};
