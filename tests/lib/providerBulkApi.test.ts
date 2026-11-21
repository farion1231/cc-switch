import { expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { providersApi } from "@/lib/api/providers";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue({}),
}));

it("passes the target app, source and preview token to the native commands", async () => {
  await providersApi.previewBulkImport("claude", "[]");
  expect(invoke).toHaveBeenCalledWith("preview_provider_bulk_import", {
    app: "claude",
    source: "[]",
  });
  await providersApi.importBulk("codex", "[]", "token");
  expect(invoke).toHaveBeenCalledWith("import_provider_bulk", {
    app: "codex",
    source: "[]",
    previewToken: "token",
  });
});
