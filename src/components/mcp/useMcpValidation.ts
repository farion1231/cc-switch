import { useTranslation } from "react-i18next";
import { validateToml, tomlToMcpServer } from "@/utils/tomlUtils";

export function useMcpValidation() {
  const { t } = useTranslation();

  // JSON basic validation (returns i18n text)
  const validateJson = (text: string): string => {
    if (!text.trim()) return "";
    try {
      const parsed = JSON.parse(text);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        return t("mcp.error.jsonInvalid");
      }
      return "";
    } catch {
      return t("mcp.error.jsonInvalid");
    }
  };

  // Unified TOML error formatting (localization + details)
  const formatTomlError = (err: string): string => {
    if (!err) return "";
    if (err === "mustBeObject" || err === "parseError") {
      return t("mcp.error.tomlInvalid");
    }
    return `${t("mcp.error.tomlInvalid")}: ${err}`;
  };

  // Full TOML validation (including required field checks)
  const validateTomlConfig = (value: string): string => {
    const err = validateToml(value);
    if (err) {
      return formatTomlError(err);
    }

    // Try to parse and check required fields
    if (value.trim()) {
      try {
        const server = tomlToMcpServer(value);
        if (server.type === "stdio" && !server.command?.trim()) {
          return t("mcp.error.commandRequired");
        }
        if (
          (server.type === "http" || server.type === "sse") &&
          !server.url?.trim()
        ) {
          return t("mcp.wizard.urlRequired");
        }
      } catch (e: any) {
        const msg = e?.message || String(e);
        return formatTomlError(msg);
      }
    }

    return "";
  };

  // Full JSON validation (including structure checks)
  const validateJsonConfig = (value: string): string => {
    const baseErr = validateJson(value);
    if (baseErr) {
      return baseErr;
    }

    // Further structure validation
    if (value.trim()) {
      try {
        const obj = JSON.parse(value);
        if (obj && typeof obj === "object") {
          if (Object.prototype.hasOwnProperty.call(obj, "mcpServers")) {
            return t("mcp.error.singleServerObjectRequired");
          }

          const typ = (obj as any)?.type;
          if (typ === "stdio" && !(obj as any)?.command?.trim()) {
            return t("mcp.error.commandRequired");
          }
          if ((typ === "http" || typ === "sse") && !(obj as any)?.url?.trim()) {
            return t("mcp.wizard.urlRequired");
          }
        }
      } catch {
        // Parse errors already covered by base validation
      }
    }

    return "";
  };

  return {
    validateJson,
    formatTomlError,
    validateTomlConfig,
    validateJsonConfig,
  };
}
