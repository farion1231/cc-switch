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

  it("maps Pi's app-level MCP refusals to localized messages", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translateMcpBackendError(
        "Pi MCP server id 'my.server' must use only letters, digits, '_' and '-'",
        t,
      ),
    ).toBe("mcp.error.piServerNameInvalid");
    expect(
      translateMcpBackendError(
        "Pi MCP server 'sse-only' uses the 'sse' transport, which Pi does not support; Pi reaches a remote server over streamable HTTP, so use that endpoint (often '/mcp' instead of '/sse') and set type to 'http'",
        t,
      ),
    ).toBe("mcp.error.piSseUnsupported");
  });
});
