import { useCallback, useEffect, useRef } from "react";

/** 始终指向最近一次渲染的值；给异步回调读最新的 props / state 用。 */
export function useLatestRef<T>(value: T) {
  const ref = useRef(value);
  useEffect(() => {
    ref.current = value;
  }, [value]);
  return ref;
}

/**
 * 受控值的「最新值 + 同步提交」：异步回调经 `ref` 读最新值；`commit` 先写 ref
 * 再调 `onChange`，紧接着的读取不必等重新渲染。`onChange` 也取最新的那个：父组件
 * 回调闭包里其余字段的旧值不会被晚到的提交带回去。
 */
export function useCommittableRef<T>(value: T, onChange: (next: T) => void) {
  const ref = useLatestRef(value);
  const onChangeRef = useLatestRef(onChange);
  const commit = useCallback(
    (next: T) => {
      ref.current = next;
      onChangeRef.current(next);
    },
    [ref, onChangeRef],
  );
  return [ref, commit] as const;
}
