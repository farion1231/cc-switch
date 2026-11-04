import { useEffect, useRef } from "react";
import { findScrollContainer, scrollToAndFlash } from "@/lib/scrollReveal";

/**
 * 全局搜索选中条目后的「定位」：先导航到条目所在的页，再在 DOM 里等目标挂上
 * （`data-search-anchor`），滚到可见处并闪两下描边。
 *
 * 目标名约定为 `<种类>:<id>`，如 `provider:abc`、`mcp:fetch`、`setting:语言`。
 * 有页内状态会挡住目标的页面（页签、筛选、应用切换）用 useRevealRequest 收到请求后自己让开。
 */

export const SEARCH_ANCHOR_ATTR = "data-search-anchor";

export interface RevealRequest {
  target: string;
  nonce: number;
}

/** 挂到条目根元素上：`<li {...searchAnchor(`mcp:${id}`)}>` */
export function searchAnchor(target: string) {
  return { [SEARCH_ANCHOR_ATTR]: target } as Record<string, string>;
}

/** 目标最多等这么久（页面切换、列表加载）；过了就算了，停在那一页。 */
const WAIT_MS = 4000;
/** 主目标等这么久还没出现、而备选已经在了，就退而定位备选。 */
const FALLBACK_AFTER_MS = 400;

let pending: RevealRequest | null = null;
let cancelActive: (() => void) | null = null;
let nonceSeed = 0;
const listeners = new Set<(request: RevealRequest) => void>();

function findAnchor(target: string): HTMLElement | null {
  const value =
    typeof CSS !== "undefined" && typeof CSS.escape === "function"
      ? CSS.escape(target)
      : target.replace(/["\\]/g, "\\$&");
  const nodes = document.querySelectorAll<HTMLElement>(
    `[${SEARCH_ANCHOR_ATTR}="${value}"]`,
  );
  // 隐藏着的那份（如挂着不卸载的另一段）不算
  for (const node of nodes) {
    if (node.getClientRects().length > 0) return node;
  }
  return null;
}

function reveal(element: HTMLElement): () => void {
  const container = findScrollContainer(element);
  if (container) return scrollToAndFlash(container, element, { inset: true });
  const animation = element.animate?.(
    [
      { boxShadow: "inset 0 0 0 2px transparent" },
      { boxShadow: "inset 0 0 0 2px hsl(var(--ring))" },
      { boxShadow: "inset 0 0 0 2px transparent" },
    ],
    { duration: 650, iterations: 2, easing: "ease-in-out" },
  );
  return () => animation?.cancel();
}

/**
 * 请求定位到某个条目。调用方先完成导航（同一次事件里 setState 即可），
 * 这里逐帧等目标出现。再次调用会取消上一次。
 */
export function requestReveal(target: string, options?: { fallback?: string }) {
  cancelActive?.();
  const request: RevealRequest = { target, nonce: ++nonceSeed };
  pending = request;
  for (const listener of listeners) listener(request);

  const startedAt = performance.now();
  let frame = 0;
  let cancelReveal: (() => void) | undefined;
  const finish = () => {
    if (pending === request) pending = null;
  };
  const tick = () => {
    const elapsed = performance.now() - startedAt;
    const element =
      findAnchor(target) ??
      (options?.fallback && elapsed >= FALLBACK_AFTER_MS
        ? findAnchor(options.fallback)
        : null);
    if (element) {
      finish();
      cancelReveal = reveal(element);
      return;
    }
    if (elapsed >= WAIT_MS) {
      finish();
      return;
    }
    frame = requestAnimationFrame(tick);
  };
  frame = requestAnimationFrame(tick);

  cancelActive = () => {
    cancelAnimationFrame(frame);
    cancelReveal?.();
    finish();
  };
}

/**
 * 页面里订阅定位请求：目标名以 `prefix` 开头时调用 handler（参数是去掉前缀的部分），
 * 让页面清掉筛选、切到对的页签。刚挂载时如果有还没完成的请求，也会补调一次。
 */
export function useRevealRequest(
  prefix: string,
  handler: (rest: string) => void,
) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    const handle = (request: RevealRequest) => {
      if (request.target.startsWith(prefix)) {
        handlerRef.current(request.target.slice(prefix.length));
      }
    };
    if (pending) handle(pending);
    listeners.add(handle);
    return () => {
      listeners.delete(handle);
    };
  }, [prefix]);
}

/** 测试用：清掉进行中的请求。 */
export function resetRevealForTests() {
  cancelActive?.();
  cancelActive = null;
  pending = null;
  listeners.clear();
}
