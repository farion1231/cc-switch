import { useState, useEffect } from "react";

export interface UseDebouncedValueOptions<T> {
  /**
   * 当满足条件时立即同步更新，不等待延迟（如输入为空或清空时）。
   */
  immediateIf?: (value: T) => boolean;
}

/**
 * 返回一个延迟更新的值，在指定时间内无新变化后才更新。
 * 用于搜索输入等场景，避免每次按键都触发请求。
 */
export function useDebouncedValue<T>(
  value: T,
  delay: number,
  options?: UseDebouncedValueOptions<T>,
): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value);

  useEffect(() => {
    if (options?.immediateIf?.(value)) {
      setDebouncedValue(value);
      return;
    }

    const timer = setTimeout(() => setDebouncedValue(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay, options?.immediateIf]);

  return debouncedValue;
}
