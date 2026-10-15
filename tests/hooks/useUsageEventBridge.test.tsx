import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import {
  QueryClient,
  QueryClientProvider,
  QueryObserver,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: events.listen }));

import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";

const key = ["usage", "summary", "all"] as const;
let client: QueryClient;
let emit: () => void;
let off: ReturnType<typeof vi.fn>;
let unsubscribe: (() => void) | undefined;

beforeEach(() => {
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  off = vi.fn();
  events.listen.mockImplementation((_event, handler) => {
    emit = () => handler({ payload: undefined });
    return Promise.resolve(off);
  });
});

afterEach(() => {
  unsubscribe?.();
  unsubscribe = undefined;
  client.clear();
});

function mountBridge() {
  return renderHook(() => useUsageEventBridge(), {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

function observeDeferredQuery(seed = true) {
  if (seed) client.setQueryData(key, 0);
  let source = 1;
  let inFlight = 0;
  let maxInFlight = 0;
  const pending: { finish: () => void; fail: () => void }[] = [];
  // Capture the database snapshot when the backend call starts. Cancelling the
  // query promise does not stop this work, just as a Tauri invoke keeps running.
  const queryFn = vi.fn(() => {
    const snapshot = source;
    inFlight += 1;
    maxInFlight = Math.max(maxInFlight, inFlight);
    return new Promise<number>((resolve, reject) => {
      pending.push({
        finish: () => resolve(snapshot),
        fail: () => reject(new Error("temporary query failure")),
      });
    }).finally(() => {
      inFlight -= 1;
    });
  });
  const observer = new QueryObserver(client, {
    queryKey: key,
    queryFn,
    staleTime: Infinity,
  });
  unsubscribe = observer.subscribe(() => {});
  return {
    queryFn,
    pending,
    setSource: (next: number) => {
      source = next;
    },
    maxInFlight: () => maxInFlight,
  };
}

describe("useUsageEventBridge", () => {
  it("invalidates only usage caches on successive backend events until unmount", async () => {
    client.setQueryData(key, 0);
    const unrelatedKey = ["providers", "claude"];
    client.setQueryData(unrelatedKey, []);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const bridge = mountBridge();

    await act(async () => emit());
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
    expect(client.getQueryState(unrelatedKey)?.isInvalidated).toBe(false);
    await act(async () => emit());
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledWith(
      { queryKey: usageKeys.all },
      { cancelRefetch: false },
    );

    bridge.unmount();
    act(() => emit());
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(off).toHaveBeenCalledTimes(1);
  });

  it("coalesces a burst into one running query and one fresh trailing query", async () => {
    const work = observeDeferredQuery();
    mountBridge();
    act(() => emit());
    expect(work.queryFn).toHaveBeenCalledTimes(1);
    work.setSource(2);
    act(() => {
      for (let index = 0; index < 20; index += 1) emit();
    });
    expect(work.queryFn).toHaveBeenCalledTimes(1);

    await act(async () => work.pending[0].finish());
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(2));
    expect(client.getQueryData(key)).toBe(1);
    await act(async () => work.pending[1].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(2));
    expect(work.maxInFlight()).toBe(1);
    expect(work.queryFn).toHaveBeenCalledTimes(2);
  });

  it("follows an already running initial fetch even when only one event arrives", async () => {
    const work = observeDeferredQuery(false);
    expect(work.queryFn).toHaveBeenCalledTimes(1);
    mountBridge();
    work.setSource(2);
    act(() => emit());
    expect(work.queryFn).toHaveBeenCalledTimes(1);

    await act(async () => work.pending[0].finish());
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(2));
    await act(async () => work.pending[1].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(2));
    expect(work.maxInFlight()).toBe(1);
  });

  it("retains events arriving during a trailing refresh without growing a queue", async () => {
    const work = observeDeferredQuery();
    mountBridge();
    act(() => emit());
    work.setSource(2);
    act(() => emit());
    await act(async () => work.pending[0].finish());
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(2));

    work.setSource(3);
    act(() => {
      for (let index = 0; index < 20; index += 1) emit();
    });
    expect(work.queryFn).toHaveBeenCalledTimes(2);
    await act(async () => work.pending[1].finish());
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(3));
    await act(async () => work.pending[2].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(3));
    expect(work.maxInFlight()).toBe(1);
    expect(work.queryFn).toHaveBeenCalledTimes(3);
  });

  it("joins an external refresh without restarting its uncancellable backend work", async () => {
    const work = observeDeferredQuery();
    mountBridge();
    const poll = client.refetchQueries({ queryKey: key });
    work.setSource(2);
    act(() => emit());
    expect(work.queryFn).toHaveBeenCalledTimes(1);

    await act(async () => {
      work.pending[0].finish();
      await poll;
    });
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(2));
    await act(async () => work.pending[1].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(2));
    expect(work.maxInFlight()).toBe(1);
  });

  it("does not add a trailing query when one event starts an idle query", async () => {
    const work = observeDeferredQuery();
    mountBridge();
    act(() => emit());
    await act(async () => work.pending[0].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(1));
    expect(work.queryFn).toHaveBeenCalledTimes(1);
  });

  it("suppresses a queued refresh and late callbacks after unmount", async () => {
    const work = observeDeferredQuery();
    const bridge = mountBridge();
    // Let the asynchronous listener registration settle before disposing it.
    await act(async () => {});
    act(() => {
      emit();
      emit();
    });
    bridge.unmount();
    await act(async () => {
      work.pending[0].finish();
    });
    act(() => emit());
    expect(work.queryFn).toHaveBeenCalledTimes(1);
    expect(off).toHaveBeenCalledTimes(1);
  });

  it("cleans up a listener whose registration finishes after unmount", async () => {
    let finishRegistration!: (unlisten: () => void) => void;
    events.listen.mockImplementation((_event, handler) => {
      emit = () => handler({ payload: undefined });
      return new Promise<() => void>((resolve) => {
        finishRegistration = resolve;
      });
    });
    const work = observeDeferredQuery();
    const bridge = mountBridge();
    bridge.unmount();
    await act(async () => finishRegistration(off));
    act(() => emit());
    expect(off).toHaveBeenCalledTimes(1);
    expect(work.queryFn).not.toHaveBeenCalled();
  });

  it("continues the queued refresh after a failed query", async () => {
    const work = observeDeferredQuery();
    mountBridge();
    act(() => {
      emit();
      emit();
    });
    work.setSource(2);
    await act(async () => work.pending[0].fail());
    await waitFor(() => expect(work.queryFn).toHaveBeenCalledTimes(2));
    await act(async () => work.pending[1].finish());
    await waitFor(() => expect(client.getQueryData(key)).toBe(2));
    expect(work.maxInFlight()).toBe(1);
  });
});
