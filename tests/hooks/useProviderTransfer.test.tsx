import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import {
  useProviderTransfer,
  encodeProviderFields,
} from "@/hooks/useProviderTransfer";
import type { ProviderTransferFields } from "@/utils/providerTransfer";
import {
  PROVIDER_TRANSFER_MIME,
  providerTransferDropTarget,
} from "@/contexts/ProviderTransferContext";
import { emitTauriEvent } from "../msw/tauriMocks";

const { extract, prepare, write, read, success, error } = vi.hoisted(() => ({
  extract: vi.fn(),
  prepare: vi.fn(),
  write: vi.fn(),
  read: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/utils/providerTransfer", () => ({
  extractProviderTransferFields: extract,
  prepareProviderTransfer: prepare,
}));
vi.mock("@/lib/clipboard", () => ({ copyText: write, readText: read }));
vi.mock("@/lib/toast", () => ({ toast: { success, error, info: vi.fn() } }));

const card: Provider = {
  id: "source",
  name: "Gateway",
  settingsConfig: { env: { ANTHROPIC_AUTH_TOKEN: "test-only-key" } },
};
const fields: ProviderTransferFields = {
  sourceApp: "claude",
  name: "Gateway",
  baseUrl: "https://example.test",
  apiKey: "test-only-key",
  models: [],
};
const draft: Provider = {
  id: "",
  name: "Gateway",
  settingsConfig: { provider: { api_key: "test-only-key" } },
};

function clipboardEvent(type: "copy" | "paste", text = "") {
  const event = new Event(type, { bubbles: true, cancelable: true });
  const data = { getData: vi.fn(() => text), setData: vi.fn() };
  Object.defineProperty(event, "clipboardData", { value: data });
  return { event, data };
}

describe("supplier field copy/paste interactions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    extract.mockReturnValue(fields);
    prepare.mockResolvedValue({
      provider: draft,
      warnings: ["Check model mapping"],
    });
    write.mockResolvedValue(undefined);
    read.mockResolvedValue(encodeProviderFields(fields));
    window.getSelection()?.removeAllRanges();
  });

  it("keeps copied fields across app navigation and only opens a target draft", async () => {
    const ready = vi.fn();
    const { result, rerender } = renderHook(
      ({ app }: { app: AppId }) =>
        useProviderTransfer({ app, enabled: true, onReady: ready }),
      { initialProps: { app: "claude" as AppId } },
    );
    await act(async () => result.current.controls.copy(card, "claude"));
    expect(write).toHaveBeenCalledWith(encodeProviderFields(fields));
    expect(ready).not.toHaveBeenCalled();
    rerender({ app: "kimi" });
    await act(async () => result.current.paste());
    expect(prepare).toHaveBeenCalledWith(fields, "kimi");
    expect(ready).toHaveBeenCalledWith(draft, ["Check model mapping"], "kimi");
    expect(card.id).toBe("source");
  });

  it("copies the selected card with the native copy event and ignores a stale selection from another app", () => {
    const { result, rerender } = renderHook(
      ({ app }: { app: AppId }) =>
        useProviderTransfer({ app, enabled: true, onReady: vi.fn() }),
      { initialProps: { app: "claude" as AppId } },
    );
    act(() => result.current.controls.select(card, "claude"));
    const first = clipboardEvent("copy");
    act(() => window.dispatchEvent(first.event));
    expect(first.event.defaultPrevented).toBe(true);
    expect(first.data.setData).toHaveBeenCalledWith(
      "text/plain",
      encodeProviderFields(fields),
    );
    rerender({ app: "codex" });
    const second = clipboardEvent("copy");
    act(() => window.dispatchEvent(second.event));
    expect(second.event.defaultPrevented).toBe(false);
  });

  it("does not intercept text input copy/paste or ordinary clipboard contents", () => {
    renderHook(() =>
      useProviderTransfer({ app: "claude", enabled: true, onReady: vi.fn() }),
    );
    const input = document.createElement("input");
    document.body.append(input);
    for (const type of ["copy", "paste"] as const) {
      const { event } = clipboardEvent(type, encodeProviderFields(fields));
      act(() => input.dispatchEvent(event));
      expect(event.defaultPrevented).toBe(false);
    }
    input.remove();
    const { event } = clipboardEvent("paste", "ordinary text");
    act(() => window.dispatchEvent(event));
    expect(event.defaultPrevented).toBe(false);
    expect(prepare).not.toHaveBeenCalled();
  });

  it("pastes system clipboard fields into the current app but stays inactive while an editor is open", async () => {
    const ready = vi.fn();
    const { rerender } = renderHook(
      ({ enabled }) =>
        useProviderTransfer({ app: "openclaw", enabled, onReady: ready }),
      { initialProps: { enabled: false } },
    );
    const first = clipboardEvent("paste", encodeProviderFields(fields));
    act(() => window.dispatchEvent(first.event));
    expect(first.event.defaultPrevented).toBe(false);
    rerender({ enabled: true });
    const second = clipboardEvent("paste", encodeProviderFields(fields));
    await act(async () => window.dispatchEvent(second.event));
    expect(prepare).toHaveBeenCalledWith(fields, "openclaw");
    expect(ready).toHaveBeenCalledOnce();
  });

  it("drops onto another app using a marker and keeps credentials out of drag data", async () => {
    const ready = vi.fn();
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "claude", enabled: true, onReady: ready }),
    );
    const data = {
      types: [PROVIDER_TRANSFER_MIME],
      setData: vi.fn(),
      effectAllowed: "",
    };
    const event = { dataTransfer: data, preventDefault: vi.fn() };
    act(() =>
      result.current.controls.startDrag(event as never, card, "claude"),
    );
    expect(data.setData).toHaveBeenCalledWith(
      PROVIDER_TRANSFER_MIME,
      "cc-switch",
    );
    expect(JSON.stringify(data.setData.mock.calls)).not.toContain(
      "test-only-key",
    );
    await act(async () => result.current.controls.drop(event as never, "pi"));
    expect(prepare).toHaveBeenCalledWith(fields, "pi");
    expect(result.current.controls.dragging).toBe(false);
    expect(ready).toHaveBeenCalledWith(draft, ["Check model mapping"], "pi");
  });

  it("reports incompatible targets without opening or saving anything", async () => {
    prepare.mockRejectedValue(new Error("Target cannot use this login"));
    const ready = vi.fn();
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "agy", enabled: true, onReady: ready }),
    );
    await act(async () => result.current.controls.copy(card, "claude"));
    await act(async () => result.current.paste());
    expect(ready).not.toHaveBeenCalled();
    expect(error).toHaveBeenCalledWith("Target cannot use this login");
    expect(result.current.busy).toBe(false);
  });

  it("uses the same target draft for a drop from the existing reorder handle", async () => {
    const ready = vi.fn();
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "claude", enabled: true, onReady: ready }),
    );
    await act(async () =>
      result.current.controls.copyTo(card, "claude", "codex"),
    );
    expect(prepare).toHaveBeenCalledWith(fields, "codex");
    expect(ready).toHaveBeenCalledWith(draft, ["Check model mapping"], "codex");
    expect(write).not.toHaveBeenCalled();
  });

  it("recognizes a sidebar drop from pointer movement and leaves keyboard sorting alone", () => {
    const button = document.createElement("button");
    button.dataset.providerDropApp = "codex";
    const icon = document.createElement("span");
    button.append(icon);
    const previous = document.elementFromPoint;
    const hit = vi.fn(() => icon);
    document.elementFromPoint = hit;
    try {
      expect(
        providerTransferDropTarget(
          new MouseEvent("pointerdown", { clientX: 100, clientY: 80 }),
          { x: -60, y: 20 },
        ),
      ).toBe("codex");
      expect(hit).toHaveBeenCalledWith(40, 100);
      expect(
        providerTransferDropTarget(new KeyboardEvent("keydown"), {
          x: 0,
          y: 20,
        }),
      ).toBeNull();
    } finally {
      document.elementFromPoint = previous;
    }
  });
});

describe("system clipboard consistency", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    extract.mockReturnValue(fields);
    prepare.mockResolvedValue({ provider: draft, warnings: [] });
    write.mockResolvedValue(undefined);
    read.mockResolvedValue(encodeProviderFields(fields));
    window.getSelection()?.removeAllRanges();
  });
  it("enables paste for a configuration copied before this view was mounted", async () => {
    read.mockResolvedValue(encodeProviderFields(fields));
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "codex", enabled: true, onReady: vi.fn() }),
    );
    await waitFor(() => expect(result.current.canPaste).toBe(true));
  });

  it("refreshes paste availability when the native window regains focus", async () => {
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "codex", enabled: true, onReady: vi.fn() }),
    );
    await waitFor(() => expect(result.current.canPaste).toBe(true));
    read.mockResolvedValue("ordinary text");
    await act(async () => emitTauriEvent("tauri://focus", null));
    expect(result.current.canPaste).toBe(false);
  });

  it("copies the current card data after an edit instead of an old selected object", async () => {
    const { result, rerender } = renderHook(
      ({ current }) =>
        useProviderTransfer({
          app: "claude",
          enabled: true,
          providers: { source: current },
          onReady: vi.fn(),
        }),
      { initialProps: { current: card } },
    );
    await act(async () => result.current.controls.select(card, "claude"));
    const updated = { ...card, name: "Updated card" };
    rerender({ current: updated });
    const { event } = clipboardEvent("copy");
    act(() => window.dispatchEvent(event));
    expect(extract).toHaveBeenLastCalledWith(updated, "claude");
  });

  it("uses the latest system clipboard instead of the last configuration copied inside the app", async () => {
    const ready = vi.fn();
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "codex", enabled: true, onReady: ready }),
    );
    await act(async () => result.current.controls.copy(card, "claude"));
    const other = { ...fields, name: "New clipboard", apiKey: "new-test-key" };
    read.mockResolvedValue(encodeProviderFields(other));
    await act(async () => result.current.paste());
    expect(prepare).toHaveBeenLastCalledWith(other, "codex");
  });

  it("does not paste cached credentials when the clipboard was replaced with ordinary text", async () => {
    const { result } = renderHook(() =>
      useProviderTransfer({ app: "codex", enabled: true, onReady: vi.fn() }),
    );
    await act(async () => result.current.controls.copy(card, "claude"));
    prepare.mockClear();
    read.mockResolvedValue("ordinary text");
    await act(async () => result.current.paste());
    expect(prepare).not.toHaveBeenCalled();
    expect(result.current.canPaste).toBe(false);
  });

  it("does not reopen a stale target after the user navigates during preparation", async () => {
    let finish!: (value: { provider: Provider; warnings: string[] }) => void;
    prepare.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    read.mockResolvedValue(encodeProviderFields(fields));
    const ready = vi.fn();
    const { result, rerender } = renderHook(
      ({ app }: { app: AppId }) =>
        useProviderTransfer({ app, enabled: true, onReady: ready }),
      { initialProps: { app: "claude" as AppId } },
    );
    act(() => result.current.controls.copyTo(card, "claude", "claude-desktop"));
    rerender({ app: "codex" });
    await act(async () => finish({ provider: draft, warnings: [] }));
    expect(ready).not.toHaveBeenCalled();
  });
});
