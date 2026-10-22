/**
 * `@tauri-apps/api/event` 的 web shim。
 *
 * `listen` 经轮询实现：对有 REST 对应的状态事件，按周期拉取并 diff，变化时回调。
 * 无 REST 对应的事件（如纯桌面端 UI 事件）→ 返回 no-op unlisten。
 */

export type UnlistenFn = () => void;

interface PollEntry {
  endpoint: () => Promise<unknown>;
  intervalMs: number;
  /** 用于判定是否变化的 key 提取。 */
  key?: (data: unknown) => string;
}

// 状态类事件 → REST 轮询端点（与 routes-map 同源）。
const POLL_MAP: Record<string, PollEntry> = {
  // provider 切换事件 → 轮当前 provider id（每个 app 轮一遍太重，这里只轮 claude）
  "provider-switched": {
    endpoint: async () => invokeRaw("/control/v1/providers/current?app=claude"),
    intervalMs: 3000,
    key: (d: any) => JSON.stringify(d),
  },
  "proxy-status-changed": {
    endpoint: async () => invokeRaw("/control/v1/proxy/status"),
    intervalMs: 2000,
    key: (d: any) => JSON.stringify(d),
  },
};

async function invokeRaw(url: string): Promise<unknown> {
  const tok = localStorage.getItem("ccs.restToken") ?? "";
  const resp = await fetch(url, {
    headers: { Authorization: `Bearer ${tok}` },
  });
  if (!resp.ok) throw new Error(`${url} ${resp.status}`);
  return resp.json();
}

export async function listen(
  event: string,
  handler: (e: { payload: unknown; event: string }) => void,
): Promise<UnlistenFn> {
  const entry = POLL_MAP[event];
  if (!entry) {
    // 无 REST 对应：web 模式无法推送，返回 no-op。
    return () => {};
  }
  let lastKey: string | undefined;
  let stopped = false;
  const tick = async () => {
    if (stopped) return;
    try {
      const data = await entry.endpoint();
      const k = entry.key ? entry.key(data) : JSON.stringify(data);
      if (k !== lastKey) {
        lastKey = k;
        handler({ event, payload: data });
      }
    } catch {
      // 单次失败静默，下周期重试。
    }
    if (!stopped) setTimeout(tick, entry.intervalMs);
  };
  setTimeout(tick, entry.intervalMs);
  return () => {
    stopped = true;
  };
}

export async function emit(): Promise<void> {
  // web 模式无后端可 emit；no-op。
}
