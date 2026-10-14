import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mutateAsyncMock = vi.fn();
const testMutateAsyncMock = vi.fn();
const scanMutateAsyncMock = vi.fn();
const followMutateAsyncMock = vi.fn();
let savedUrl: string | null = "http://127.0.0.1:7890";
let followSystemProxy: boolean | undefined = true;
let followPending = false;

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({
    data: savedUrl,
    isLoading: false,
  }),
  useSetGlobalProxyUrl: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
  useTestProxy: () => ({
    mutateAsync: testMutateAsyncMock,
    isPending: false,
  }),
  useScanProxies: () => ({
    mutateAsync: scanMutateAsyncMock,
    isPending: false,
  }),
  useFollowSystemProxy: () => ({ data: followSystemProxy }),
  useSetFollowSystemProxy: () => ({
    mutateAsync: followMutateAsyncMock,
    isPending: followPending,
  }),
}));

describe("GlobalProxySettings", () => {
  beforeEach(() => {
    mutateAsyncMock.mockReset();
    testMutateAsyncMock.mockReset();
    scanMutateAsyncMock.mockReset();
    followMutateAsyncMock.mockReset().mockResolvedValue(undefined);
    savedUrl = "http://127.0.0.1:7890";
    followSystemProxy = true;
    followPending = false;
  });

  it("renders proxy URL input with saved value", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );
    // URL 对象会在末尾添加斜杠
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890/"));
  });

  it("saves proxy URL when save button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    fireEvent.change(urlInput, { target: { value: "http://localhost:8080" } });

    const saveButton = screen.getByRole("button", { name: "common.save" });
    fireEvent.click(saveButton);

    await waitFor(() => expect(mutateAsyncMock).toHaveBeenCalled());
    // 没有用户名时，URL 不经过 URL 对象解析，所以没有尾部斜杠
    expect(mutateAsyncMock).toHaveBeenCalledWith("http://localhost:8080");
  });

  it("clears proxy URL when clear button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    // Wait for initial value to load
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890/"));

    // Click clear button
    const clearButton = screen.getByRole("button", {
      name: "settings.globalProxy.clear",
    });
    fireEvent.click(clearButton);

    expect(urlInput).toHaveValue("");
  });

  it("lets users explicitly disable system proxy following without a saved proxy", async () => {
    savedUrl = null;
    render(<GlobalProxySettings />);
    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.globalProxy.followSystemProxy",
      }),
    );
    await waitFor(() =>
      expect(followMutateAsyncMock).toHaveBeenCalledWith(false),
    );
  });

  it("keeps following disabled until an explicit proxy is actually cleared and saved", () => {
    render(<GlobalProxySettings />);
    fireEvent.click(
      screen.getByRole("button", { name: "settings.globalProxy.clear" }),
    );
    expect(screen.getByRole("switch")).toBeDisabled();
    expect(followMutateAsyncMock).not.toHaveBeenCalled();
  });

  it("shows the persisted direct policy, not an unsaved proxy draft", () => {
    savedUrl = null;
    followSystemProxy = false;
    render(<GlobalProxySettings />);
    fireEvent.change(
      screen.getByPlaceholderText(
        "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
      ),
      { target: { value: "http://localhost:8080" } },
    );
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(
      "settings.globalProxy.directStatus",
    );
  });

  it("disables the switch while loading or saving", () => {
    savedUrl = null;
    followSystemProxy = undefined;
    const { rerender } = render(<GlobalProxySettings />);
    expect(screen.getByRole("switch")).toBeDisabled();
    followSystemProxy = true;
    followPending = true;
    rerender(<GlobalProxySettings />);
    expect(screen.getByRole("switch")).toBeDisabled();
  });

  it("keeps the saved following policy visible when saving direct mode fails", async () => {
    savedUrl = null;
    followMutateAsyncMock.mockRejectedValue(new Error("save failed"));
    render(<GlobalProxySettings />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(followMutateAsyncMock).toHaveBeenCalledWith(false),
    );
    expect(screen.getByRole("switch")).toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(
      "settings.globalProxy.followStatus",
    );
  });
});
