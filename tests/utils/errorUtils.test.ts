import { describe, expect, it, vi } from "vitest";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
} from "@/utils/errorUtils";

describe("error utilities", () => {
  it("extracts Tauri string errors", () => {
    expect(extractErrorMessage("backend failed")).toBe("backend failed");
  });

  it.each([
    ["Pi provider 'anthropic' is managed by Pi", "pi.provider.managedByPi"],
    [
      "无效输入: Pi is currently using model 'model-a'; choose another model in Pi before removing it",
      "pi.form.currentModelMustRemain",
    ],
    [
      "Pi provider 'custom' changed outside CC Switch",
      "pi.provider.configChanged",
    ],
  ])("maps Pi provider conflicts to %s", (message, expectedKey) => {
    const t = vi.fn((key: string) => key);

    expect(translatePiProviderMutationError(message, t)).toBe(expectedKey);
  });
});
