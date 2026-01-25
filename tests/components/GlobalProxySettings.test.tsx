import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mutateAsyncMock = vi.fn();
const testMutateAsyncMock = vi.fn();
const scanMutateAsyncMock = vi.fn();

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({ data: "http://127.0.0.1:7890", isLoading: false }),
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
}));

describe("GlobalProxySettings", () => {
  beforeEach(() => {
    mutateAsyncMock.mockReset();
    testMutateAsyncMock.mockReset();
    scanMutateAsyncMock.mockReset();
  });

  it("renders proxy URL input with saved value", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );
    // URL 对象会在末尾添加斜杠
    await waitFor(() =>
      expect(urlInput).toHaveValue("http://127.0.0.1:7890/"),
    );
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
    await waitFor(() =>
      expect(urlInput).toHaveValue("http://127.0.0.1:7890/"),
    );

    // Click clear button
    const clearButton = screen.getByTitle("settings.globalProxy.clear");
    fireEvent.click(clearButton);

    expect(urlInput).toHaveValue("");
  });
});
