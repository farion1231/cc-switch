import { useState, useCallback, useEffect, useRef } from "react";
import TOML from "smol-toml";

/**
 * Codex config.toml 格式校验 Hook
 * 使用 smol-toml 进行实时 TOML 语法校验（带 debounce）
 */
export function useCodexTomlValidation() {
  const [configError, setConfigError] = useState("");
  const debounceTimerRef = useRef<NodeJS.Timeout | null>(null);

  /**
   * 校验 TOML 格式
   * @param tomlText - 待校验的 TOML 文本
   * @returns 是否校验通过
   */
  const validateToml = useCallback((tomlText: string): boolean => {
    // 空字符串视为合法（允许为空）
    if (!tomlText.trim()) {
      setConfigError("");
      return true;
    }

    try {
      TOML.parse(tomlText);
      setConfigError("");
      return true;
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : "TOML 格式错误";
      setConfigError(errorMessage);
      return false;
    }
  }, []);

  /**
   * 带 debounce 的校验函数（500ms 延迟）
   * @param tomlText - 待校验的 TOML 文本
   */
  const debouncedValidate = useCallback(
    (tomlText: string) => {
      // 清除之前的定时器
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }

      // 设置新的定时器
      debounceTimerRef.current = setTimeout(() => {
        validateToml(tomlText);
      }, 500);
    },
    [validateToml],
  );

  /**
   * 清空错误信息
   */
  const clearError = useCallback(() => {
    setConfigError("");
  }, []);

  // 清理定时器
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  return {
    configError,
    validateToml,
    debouncedValidate,
    clearError,
  };
}
