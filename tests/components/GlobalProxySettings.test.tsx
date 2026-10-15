import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mutateAsyncMock = vi.fn();
const testMutateAsyncMock = vi.fn();
const scanMutateAsyncMock = vi.fn();
let savedProxyUrl = "http://127.0.0.1:7890";

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({
    data: savedProxyUrl,
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
}));

describe("GlobalProxySettings", () => {
  beforeEach(() => {
    savedProxyUrl = "http://127.0.0.1:7890";
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

  it.each([
    ["user%name", "pass%word"],
    ["user%40name", "pass%3Aword"],
    ["user", "100%"],
    ["user@name", "p@ss:word/#? "],
    ["\u7528\u6237", "\u5bc6\u7801%25"],
  ])(
    "preserves credentials %s / %s when saved and reloaded",
    async (username, password) => {
      const { unmount } = render(<GlobalProxySettings />);

      fireEvent.change(
        screen.getByPlaceholderText("settings.globalProxy.username"),
        {
          target: { value: username },
        },
      );
      fireEvent.change(
        screen.getByPlaceholderText("settings.globalProxy.password"),
        {
          target: { value: password },
        },
      );
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));

      await waitFor(() => expect(mutateAsyncMock).toHaveBeenCalledTimes(1));
      savedProxyUrl = mutateAsyncMock.mock.calls[0][0];
      unmount();
      render(<GlobalProxySettings />);

      expect(
        screen.getByPlaceholderText("settings.globalProxy.username"),
      ).toHaveValue(username);
      expect(
        screen.getByPlaceholderText("settings.globalProxy.password"),
      ).toHaveValue(password);
      expect(
        screen.getByPlaceholderText(
          "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
        ),
      ).toHaveValue("http://127.0.0.1:7890/");
    },
  );
});
