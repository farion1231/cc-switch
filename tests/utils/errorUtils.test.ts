import { describe, expect, it, vi } from "vitest";
import {
  extractErrorMessage,
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
});
