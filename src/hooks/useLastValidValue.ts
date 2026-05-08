import { useRef } from "react";

/**
 * 保存最后一个非 null/undefined 的值
 * 用于 Dialog 关闭动画期间保持内容显示
 *
 * @param value 当前值
 * @returns 当前值（如果有效）或最后一个有效值
 */
export function useLastValidValue<T>(value: T | null | undefined): T | null {
  const ref = useRef<T | null>(null);

  // 同步更新 ref（在渲染期间，不在 useEffect 中）
  if (value != null) {
    ref.current = value;
  }

  // 返回当前值或最后有效值
  return value ?? ref.current;
}
