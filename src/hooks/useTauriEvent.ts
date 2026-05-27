import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * 在 useEffect 中监听 Tauri 事件，自动管理异步注册和卸载清理。
 * 避免每次使用时重复编写 active flag + async setup 样板代码。
 */
export function useTauriEvent<P>(
  eventName: string,
  handler: (payload: P) => void | Promise<void>,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    void (async () => {
      try {
        const off = await listen<P>(eventName, (event) => {
          void handlerRef.current(event.payload);
        });
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      } catch (error) {
        console.error(`Failed to subscribe ${eventName} event`, error);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [eventName]);
}
