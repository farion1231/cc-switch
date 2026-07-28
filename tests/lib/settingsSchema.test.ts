import { describe, expect, it } from "vitest";
import { settingsSchema } from "@/lib/schemas/settings";

describe("settingsSchema webdav legacy module parsing", () => {
  it("accepts partially present legacy module objects", () => {
    const parsed = settingsSchema.parse({
      showInTray: true,
      minimizeToTrayOnClose: true,
      webdavSync: {
        uploadModules: {
          api: false,
        },
        downloadModules: {
          api: true,
          mcp: false,
        },
      },
    });

    expect(parsed.webdavSync?.uploadModules).toEqual({ api: false });
    expect(parsed.webdavSync?.downloadModules).toEqual({
      api: true,
      mcp: false,
    });
  });
});
