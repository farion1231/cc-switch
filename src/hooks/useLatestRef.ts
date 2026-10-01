import { useEffect, useRef } from "react";

/** 始终指向最近一次渲染的值；给异步回调读最新的 props / state 用。 */
export function useLatestRef<T>(value: T) {
  const ref = useRef(value);
  useEffect(() => {
    ref.current = value;
  }, [value]);
  return ref;
}
