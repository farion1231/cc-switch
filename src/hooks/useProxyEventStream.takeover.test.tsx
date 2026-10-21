import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { useProxyEventStream } from "./useProxyEventStream";

// 设置查询会走 Tauri IPC，测试里只关心端口与总开关这两个字段。
//
// `vi.hoisted` 是必须的：`vi.mock` 的工厂会被提升到 import 之前求值，直接引用
// 模块顶层的 `let` 只会拿到 TDZ。
const { mockSettings } = vi.hoisted(() => ({
  mockSettings: { remotePort: 4200, remoteEnabled: true } as {
    remotePort: number;
    remoteEnabled: boolean;
  },
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mockSettings }),
}));

// 键值本身不是本用例的被测对象，但真实模块会拉进 api 层（jsdom 里没有 Tauri）。
vi.mock("@/lib/query/proxy", () => ({
  proxyKeys: {
    status: ["proxy", "status"],
    takeoverStatus: ["proxy", "takeoverStatus"],
  },
}));

class MockEventSource {
  static instances: MockEventSource[] = [];
  url: string;
  onmessage: ((event: MessageEvent) => void) | null = null;
  close = vi.fn();

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }
}

function setup() {
  const client = new QueryClient();
  const invalidate = vi
    .spyOn(client, "invalidateQueries")
    .mockImplementation(async () => undefined);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { unmount } = renderHook(() => useProxyEventStream(), { wrapper });
  return { invalidate, unmount };
}

function emit(es: MockEventSource, payload: unknown) {
  es.onmessage?.(
    new MessageEvent("message", { data: JSON.stringify(payload) }),
  );
}

/** 依次取回每次失效的 queryKey，供顺序断言使用。 */
function invalidatedKeys(
  invalidate: ReturnType<typeof setup>["invalidate"],
): unknown[] {
  return invalidate.mock.calls.map((call) => call[0]);
}

describe("useProxyEventStream — takeover_changed", () => {
  beforeEach(() => {
    MockEventSource.instances = [];
    mockSettings.remotePort = 4200;
    mockSettings.remoteEnabled = true;
    vi.stubGlobal(
      "EventSource",
      MockEventSource as unknown as typeof EventSource,
    );
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  // 远程改接管模式会改「当前供应商」：只失效 takeoverStatus 时，桌面 UI 的
  // 供应商列表仍是旧值，远程明明已经切了模式，本地却显示没切。
  it("takeover_changed_invalidates_providers", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "takeover_changed", app_type: "claude", enabled: true });

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["proxy", "takeoverStatus"],
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers"] });
    expect(invalidatedKeys(invalidate)).toEqual([
      { queryKey: ["proxy", "takeoverStatus"] },
      { queryKey: ["providers"] },
    ]);
  });

  // live_configs_changed / shutdown 的行为不变：只失效接管状态。
  it("live_configs_changed 帧只失效接管状态", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "live_configs_changed" });

    expect(invalidatedKeys(invalidate)).toEqual([
      { queryKey: ["proxy", "takeoverStatus"] },
    ]);
  });

  it("shutdown 帧只失效接管状态", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "shutdown" });

    expect(invalidatedKeys(invalidate)).toEqual([
      { queryKey: ["proxy", "takeoverStatus"] },
    ]);
  });
});
