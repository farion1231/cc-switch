import type { ReactNode } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getSnapshotMock, listTargetsMock, onStatusMock, setActiveTargetMock } =
  vi.hoisted(() => ({
    getSnapshotMock: vi.fn(),
    listTargetsMock: vi.fn(),
    onStatusMock: vi.fn(),
    setActiveTargetMock: vi.fn(),
  }));

vi.mock("@/lib/api/remote", () => ({
  remoteApi: {
    getSnapshot: getSnapshotMock,
    listTargets: listTargetsMock,
    onStatus: onStatusMock,
    setActiveTarget: setActiveTargetMock,
  },
}));

import {
  RuntimeTargetProvider,
  useRuntimeTarget,
} from "./RuntimeTargetContext";
import { setRuntimeSnapshot } from "@/lib/runtime/store";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function RuntimeProbe() {
  const runtime = useRuntimeTarget();
  return (
    <button
      type="button"
      onClick={() => void runtime.setActiveTarget("server-a")}
    >
      {runtime.snapshot.status}
    </button>
  );
}

describe("RuntimeTargetProvider query lifecycle", () => {
  beforeEach(() => {
    getSnapshotMock.mockReset();
    listTargetsMock.mockReset();
    onStatusMock.mockReset();
    setActiveTargetMock.mockReset();
    getSnapshotMock.mockResolvedValue({ status: "local", generation: 0 });
    listTargetsMock.mockResolvedValue([]);
    onStatusMock.mockResolvedValue(() => undefined);
    setRuntimeSnapshot({ status: "local", generation: 0 });
  });

  it("cancels old queries before transition and clears environment caches before online", async () => {
    const cancelGate = deferred<void>();
    const connectGate = deferred<{
      status: "online";
      generation: number;
      activeTargetId: string;
    }>();
    const queryClient = new QueryClient();
    const cancelSpy = vi
      .spyOn(queryClient, "cancelQueries")
      .mockReturnValueOnce(cancelGate.promise);
    const removeSpy = vi.spyOn(queryClient, "removeQueries");
    setActiveTargetMock.mockReturnValue(connectGate.promise);

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    render(
      <RuntimeTargetProvider>
        <RuntimeProbe />
      </RuntimeTargetProvider>,
      { wrapper },
    );

    await userEvent.click(screen.getByRole("button", { name: "local" }));

    expect(cancelSpy).toHaveBeenCalledTimes(1);
    expect(setActiveTargetMock).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "local" })).toBeInTheDocument();

    await act(async () => cancelGate.resolve());
    await waitFor(() =>
      expect(setActiveTargetMock).toHaveBeenCalledWith("server-a"),
    );
    expect(
      screen.getByRole("button", { name: "connecting" }),
    ).toBeInTheDocument();

    await act(async () =>
      connectGate.resolve({
        status: "online",
        generation: 1,
        activeTargetId: "server-a",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "online" }),
      ).toBeInTheDocument(),
    );
    expect(removeSpy).toHaveBeenCalledWith({ queryKey: ["providers"] });
    expect(removeSpy).toHaveBeenCalledWith({ queryKey: ["usage"] });
  });
});
