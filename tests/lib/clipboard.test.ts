import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const isLinuxMock = vi.fn(() => false);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isLinux: () => isLinuxMock(),
}));

import { copyText } from "@/lib/clipboard";

const writeTextMock = vi.fn();

beforeEach(() => {
  invokeMock.mockReset().mockResolvedValue(true);
  writeTextMock.mockReset().mockResolvedValue(undefined);
  isLinuxMock.mockReset().mockReturnValue(false);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
});

describe("copyText", () => {
  it("uses the native clipboard command first off Linux", async () => {
    await copyText("hello");
    expect(invokeMock).toHaveBeenCalledWith("copy_text_to_clipboard", {
      text: "hello",
    });
    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("falls back to the web clipboard when the native command fails", async () => {
    invokeMock.mockRejectedValue(new Error("native failed"));
    await copyText("hello");
    expect(writeTextMock).toHaveBeenCalledWith("hello");
  });

  it("uses the web clipboard first on Linux (#9384)", async () => {
    isLinuxMock.mockReturnValue(true);
    await copyText("code");
    expect(writeTextMock).toHaveBeenCalledWith("code");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("falls back to the native command on Linux when the web clipboard fails", async () => {
    isLinuxMock.mockReturnValue(true);
    writeTextMock.mockRejectedValue(new Error("denied"));
    await copyText("code");
    expect(invokeMock).toHaveBeenCalledWith("copy_text_to_clipboard", {
      text: "code",
    });
  });

  it("throws when both paths fail", async () => {
    invokeMock.mockRejectedValue(new Error("native failed"));
    writeTextMock.mockRejectedValue(new Error("web failed"));
    await expect(copyText("x")).rejects.toThrow("web failed");
  });
});
