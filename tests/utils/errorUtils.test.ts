import { describe, expect, it, vi } from "vitest";
import {
  extractErrorMessage,
  translatePiProviderMutationError,
  translateOhMyPiProviderMutationError,
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

  it("maps a simultaneous Oh My Pi models.yml write to a concise error", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translateOhMyPiProviderMutationError(
        "Oh My Pi models changed outside CC Switch",
        t,
      ),
    ).toBe("ohmypi.provider.writeConflict");
  });

  it("maps a duplicate Oh My Pi provider key to validation feedback", () => {
    const t = vi.fn((key: string) => key);

    expect(
      translateOhMyPiProviderMutationError(
        "无效输入: Oh My Pi provider key 'duplicate' already exists in models.yml",
        t,
      ),
    ).toBe("ohmypi.form.providerKeyDuplicate");
  });
});
