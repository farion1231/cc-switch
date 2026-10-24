import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { useProxyEventStream } from "./useProxyEventStream";

// 设置查询会走 Tauri IPC，测试里只关心端口与总开关这两个字段。
//
// 默认 `remoteEnabled: true`：其余用例都在验「订阅建起来了之后收到某类帧会失效
// 哪些缓存」，而 hook 现在对关闭态直接 return —— 默认不显式写上这个字段的话，
// 那些用例会因为「一个 EventSource 都没建」而集体报 undefined，连带掩盖真正的
// 帧处理回归。要验关闭态的用例显式覆写 `mockSettings`。
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

describe("useProxyEventStream", () => {
  beforeEach(() => {
    MockEventSource.instances = [];
    // 每个用例从「远程管理开着」这个基线出发：改写 mockSettings 的用例自己负责
    // 在断言前改，避免关闭态漏回下一个用例。
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

  it("订阅用户配置的远程端口，而不是写死 4000", () => {
    setup();
    expect(MockEventSource.instances[0]?.url).toBe(
      "http://127.0.0.1:4200/api/events",
    );
  });

  it("远程管理开着的时候照常订阅（正向对照）", () => {
    mockSettings.remoteEnabled = true;
    setup();
    expect(MockEventSource.instances).toHaveLength(1);
  });

  it("远程管理关着的时候一个 EventSource 都不建", () => {
    mockSettings.remoteEnabled = false;
    setup();
    expect(MockEventSource.instances).toHaveLength(0);
  });

  it("reorder 帧失效 providers 与 failoverQueue 两份缓存", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "reorder", degraded: true });

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["failoverQueue"] });
  });

  it("reorder 帧不误伤代理状态查询", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "reorder", degraded: true });

    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ["proxy", "takeoverStatus"],
    });
  });

  it("非 JSON 帧不会抛错，也不会触发失效", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    expect(() =>
      es.onmessage?.(new MessageEvent("message", { data: "not json" })),
    ).not.toThrow();
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("switch 帧失效供应商、队列、开关与接管状态四份缓存", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "switch", degraded: true });

    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["failoverQueue"] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["autoFailoverEnabled"],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["proxy", "takeoverStatus"],
    });
  });

  it("failover_changed 帧失效开关、队列、供应商与代理状态五份缓存", () => {
    const { invalidate } = setup();
    const es = MockEventSource.instances[0]!;

    emit(es, { type: "failover_changed", enabled: true });

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["autoFailoverEnabled"],
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["failoverQueue"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["proxy", "status"] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["proxy", "takeoverStatus"],
    });
  });
});
