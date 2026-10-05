import { describe, expect, it, vi } from "vitest";
import {
  extractErrorMessage,
  translateMcpBackendError,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";

describe("error utilities", () => {
  it("extracts Tauri string errors", () => {
    expect(extractErrorMessage("backend failed")).toBe("backend failed");
  });

  it("maps a simultaneous models.json write to a concise error", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translatePiProviderMutationError(
        "Pi models.json changed outside CC Switch",
        t,
      ),
    ).toBe("pi.provider.writeConflict");
  });

  it("maps a duplicate Pi provider key to validation feedback", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translatePiProviderMutationError(
        "无效输入: Pi provider key 'duplicate' already exists in models.json",
        t,
      ),
    ).toBe("pi.form.providerKeyDuplicate");
  });

  it("maps the MCP servers Pi refuses to localized messages", () => {
    const t = vi.fn(
      (key: string, opts?: Record<string, unknown>) =>
        `${key}${opts ? JSON.stringify(opts) : ""}`,
    );

    expect(
      translateMcpBackendError(
        "MCP 校验失败: Pi 不支持 SSE 连接方式的 MCP 服务器，请改用 HTTP（streamable HTTP）",
        t,
      ),
    ).toBe("mcp.error.piSseUnsupported");
    expect(
      translateMcpBackendError(
        "MCP 校验失败: Pi 的 MCP 服务器名只能包含字母、数字、_ 和 -：bad name",
        t,
      ),
    ).toBe('mcp.error.piInvalidName{"id":"bad name"}');
    expect(
      translateMcpBackendError(
        "MCP 校验失败: Pi 把只差 - 和 _ 的服务器名视为同一个：dev_tools 与已有的 dev-tools 冲突",
        t,
      ),
    ).toBe('mcp.error.piNameConflict{"id":"dev_tools","other":"dev-tools"}');
  });
});
