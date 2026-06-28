import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));

const jsonResponse = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });

async function importUpdater() {
  vi.resetModules();
  return import("@/lib/updater");
}

describe("updater", () => {
  let originalTauri: unknown;

  beforeEach(() => {
    vi.restoreAllMocks();
    // @ts-ignore - Tests run with Tauri globals enabled by default
    originalTauri = window.__TAURI__;
    // @ts-ignore
    delete window.__TAURI__;
  });

  afterEach(() => {
    // @ts-ignore
    window.__TAURI__ = originalTauri;
  });

  describe("web mode", () => {
    it("getCurrentVersion returns version from /health", async () => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        jsonResponse({ status: "healthy", version: "3.16.3" }),
      );

      const { getCurrentVersion } = await importUpdater();
      const version = await getCurrentVersion();

      expect(version).toBe("3.16.3");
      expect(fetch).toHaveBeenCalledWith("/health");
    });

    it("getCurrentVersion returns empty string when /health fails", async () => {
      vi.spyOn(globalThis, "fetch").mockRejectedValue(
        new Error("Network error"),
      );

      const { getCurrentVersion } = await importUpdater();
      const version = await getCurrentVersion();

      expect(version).toBe("");
    });

    it("checkForUpdate returns up-to-date when versions match", async () => {
      vi.spyOn(globalThis, "fetch").mockImplementation(async (url) => {
        if (url === "/health") {
          return jsonResponse({ status: "healthy", version: "3.16.3" });
        }
        if (String(url).includes("api.github.com")) {
          return jsonResponse({ tag_name: "v3.16.3" });
        }
        throw new Error(`Unexpected request: ${String(url)}`);
      });

      const { checkForUpdate } = await importUpdater();
      const result = await checkForUpdate();

      expect(result).toEqual({ status: "up-to-date" });
    });

    it("checkForUpdate returns available when a newer version exists", async () => {
      vi.spyOn(globalThis, "fetch").mockImplementation(async (url) => {
        if (url === "/health") {
          return jsonResponse({ status: "healthy", version: "3.16.3" });
        }
        if (String(url).includes("api.github.com")) {
          return jsonResponse({
            tag_name: "v3.17.0",
            body: "Release notes",
            published_at: "2026-06-01T00:00:00Z",
          });
        }
        throw new Error(`Unexpected request: ${String(url)}`);
      });

      const { checkForUpdate } = await importUpdater();
      const result = await checkForUpdate();

      expect(result.status).toBe("available");
      if (result.status === "available") {
        expect(result.info).toMatchObject({
          currentVersion: "3.16.3",
          availableVersion: "3.17.0",
          notes: "Release notes",
          pubDate: "2026-06-01T00:00:00Z",
        });
      }
    });

    it("checkForUpdate returns up-to-date when GitHub request fails", async () => {
      vi.spyOn(globalThis, "fetch").mockImplementation(async (url) => {
        if (url === "/health") {
          return jsonResponse({ status: "healthy", version: "3.16.3" });
        }
        throw new Error("Network error");
      });

      const { checkForUpdate } = await importUpdater();
      const result = await checkForUpdate();

      expect(result).toEqual({ status: "up-to-date" });
    });
  });
});
