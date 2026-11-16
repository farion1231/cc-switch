import { describe, expect, it, vi } from "vitest";
import {
  extractErrorMessage,
  translateAtomcodeProviderMutationError,
  translateMcpBackendError,
  translateKimiProviderMutationError,
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

  it("explains Kimi removals refused because Kimi Code still uses the model", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translateKimiProviderMutationError(
        "无效输入: Select another default model in Kimi Code before removing this model or provider",
        t,
      ),
    ).toBe("kimi.error.defaultModelInUse");
    expect(
      translateKimiProviderMutationError(
        "无效输入: Update the secondary model selection in Kimi Code before removing this model or provider",
        t,
      ),
    ).toBe("kimi.error.secondaryModelInUse");
    expect(translateKimiProviderMutationError("other failure", t)).toBe("");
  });

  it("maps AtomCode backend mutation errors to i18n keys", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translateAtomcodeProviderMutationError(
        "Cannot delete or rename the default model 'acct-key-a' while it is selected",
        t,
      ),
    ).toBe("atomcode.error.defaultModelInUse");
    expect(
      translateAtomcodeProviderMutationError(
        "Select another provider in AtomCode before removing this one",
        t,
      ),
    ).toBe("atomcode.error.removeSelected");
    expect(
      translateAtomcodeProviderMutationError(
        "This provider comes from a legacy [providers.*] configuration; run AtomCode once to migrate it, then remove it",
        t,
      ),
    ).toBe("atomcode.error.legacyRemoveUnsupported");
    expect(
      translateAtomcodeProviderMutationError(
        "AtomCode account key 'acct' already exists",
        t,
      ),
    ).toBe("atomcode.error.accountKeyExists");
    expect(
      translateAtomcodeProviderMutationError("Enter an AtomCode account key", t),
    ).toBe("atomcode.error.accountKeyRequired");
    expect(
      translateAtomcodeProviderMutationError(
        "AtomCode account key may only contain letters, digits, '-', '_' or '.'",
        t,
      ),
    ).toBe("atomcode.error.accountKeyInvalid");
    expect(
      translateAtomcodeProviderMutationError(
        "Select a supported AtomCode provider",
        t,
      ),
    ).toBe("atomcode.error.providerUnsupported");
    expect(
      translateAtomcodeProviderMutationError(
        "Enter a valid AtomCode endpoint URL",
        t,
      ),
    ).toBe("atomcode.error.endpointInvalid");
    expect(
      translateAtomcodeProviderMutationError(
        "Add at least one AtomCode model",
        t,
      ),
    ).toBe("atomcode.error.noModels");
    expect(
      translateAtomcodeProviderMutationError(
        "The AtomCode provider has no models",
        t,
      ),
    ).toBe("atomcode.error.noModels");
    expect(
      translateAtomcodeProviderMutationError(
        "AtomCode models need a key, model ID and matching account",
        t,
      ),
    ).toBe("atomcode.error.modelInvalid");
    expect(
      translateAtomcodeProviderMutationError(
        "AtomCode model context_window must be positive",
        t,
      ),
    ).toBe("atomcode.error.contextWindowInvalid");
    expect(
      translateAtomcodeProviderMutationError(
        "AtomCode model key 'm' belongs to another provider",
        t,
      ),
    ).toBe("atomcode.error.modelKeyBelongsToAnother");
    expect(
      translateAtomcodeProviderMutationError(
        "Invalid AtomCode TOML configuration",
        t,
      ),
    ).toBe("atomcode.error.tomlInvalid");
    // 未识别的消息回退原文（返回空串由调用方兜底）
    expect(translateAtomcodeProviderMutationError("other failure", t)).toBe("");
  });
});
