import * as React from "react";
import { useEnterTransition, type EnterFrom } from "@/hooks/useEnterTransition";

interface EnterTransitionProps<K> extends React.HTMLAttributes<HTMLDivElement> {
  /** 变了就播一次进场 */
  transitionKey: K;
  from?: EnterFrom | ((prev: K | undefined, next: K) => EnterFrom);
  /** 默认 true：随页面一起挂载时不播，外层整页已经在动了 */
  skipInitial?: boolean;
}

/** useEnterTransition 的 div 版：直接替换原来那层容器，不多套一层。 */
export function EnterTransition<K>({
  transitionKey,
  from,
  skipInitial = true,
  ...props
}: EnterTransitionProps<K>) {
  const ref = useEnterTransition<HTMLDivElement, K>(transitionKey, {
    from,
    skipInitial,
  });
  return <div ref={ref} {...props} />;
}
