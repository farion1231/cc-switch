import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { readText } from "@/lib/clipboard";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
afterEach(() => {
  vi.clearAllMocks();
  if (original) Object.defineProperty(navigator, "clipboard", original);
  else Reflect.deleteProperty(navigator, "clipboard");
});

describe("system clipboard reading", () => {
  it("uses the native clipboard even when its current content has no text", async () => {
    const browserRead = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: browserRead },
    });
    vi.mocked(invoke).mockResolvedValue("");
    expect(await readText()).toBe("");
    expect(invoke).toHaveBeenCalledWith("read_text_from_clipboard");
    expect(browserRead).not.toHaveBeenCalled();
  });

  it("uses browser clipboard access when the native command is unavailable", async () => {
    const browserRead = vi.fn().mockResolvedValue("current clipboard");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: browserRead },
    });
    vi.mocked(invoke).mockRejectedValue(new Error("not a native window"));
    expect(await readText()).toBe("current clipboard");
  });

  it("reports denied access instead of returning an old configuration", async () => {
    const denied = new Error("clipboard access denied");
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: vi.fn().mockRejectedValue(denied) },
    });
    vi.mocked(invoke).mockRejectedValue(new Error("native access failed"));
    await expect(readText()).rejects.toBe(denied);
  });
});
