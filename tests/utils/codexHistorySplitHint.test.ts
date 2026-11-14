import { describe, it, expect, vi, beforeEach } from "vitest";
import type { TFunction } from "i18next";
import {
  codexHistorySplitToastExtras,
  showCodexUnifyMigrationFinished,
} from "@/utils/codexHistorySplitHint";

const warningMock = vi.fn();
const successMock = vi.fn();
const errorMock = vi.fn();
vi.mock("@/lib/toast", () => ({
  toast: {
    warning: (...args: unknown[]) => warningMock(...args),
    success: (...args: unknown[]) => successMock(...args),
    error: (...args: unknown[]) => errorMock(...args),
  },
}));

const t = ((key: string, options?: Record<string, unknown>) =>
  options && "count" in options
    ? `${key}:${String(options.previous)}:${String(options.count)}`
    : key) as unknown as TFunction;

describe("codexHistorySplitToastExtras", () => {
  beforeEach(() => {
    warningMock.mockReset();
    successMock.mockReset();
    errorMock.mockReset();
  });

  it("is one line on the success toast naming the locked group, without a button", () => {
    const extras = codexHistorySplitToastExtras(
      { previousBucket: "openai", currentBucket: "custom", lockedThreads: 12 },
      t,
    );
    expect(extras).toEqual({
      description:
        "notifications.codexHistorySplit.hint:notifications.codexHistorySplit.officialBucket:12",
      duration: 6000,
    });
    expect(warningMock).not.toHaveBeenCalled();
    expect(successMock).not.toHaveBeenCalled();

    const other = codexHistorySplitToastExtras(
      { previousBucket: "custom", currentBucket: "openai", lockedThreads: 3 },
      t,
    );
    expect(other.description).toContain("thirdPartyBucket:3");
  });
});

describe("showCodexUnifyMigrationFinished", () => {
  beforeEach(() => {
    successMock.mockReset();
    errorMock.mockReset();
  });

  it("tells the user how many sessions moved so they can restart Codex", () => {
    showCodexUnifyMigrationFinished(
      { migratedStateRows: 3986, migratedJsonlFiles: 3732, error: null },
      t,
    );
    expect(successMock).toHaveBeenCalledWith(
      "notifications.codexHistorySplit.migrationFinished:undefined:3986",
      expect.anything(),
    );
    expect(errorMock).not.toHaveBeenCalled();
  });

  it("falls back to the session file count when no state DB was read", () => {
    showCodexUnifyMigrationFinished(
      { migratedStateRows: 0, migratedJsonlFiles: 7 },
      t,
    );
    expect(successMock).toHaveBeenCalledWith(
      "notifications.codexHistorySplit.migrationFinished:undefined:7",
      expect.anything(),
    );
  });

  it("stays quiet when nothing was migrated", () => {
    showCodexUnifyMigrationFinished(
      { migratedStateRows: 0, migratedJsonlFiles: 0 },
      t,
    );
    expect(successMock).not.toHaveBeenCalled();
    expect(errorMock).not.toHaveBeenCalled();
  });

  it("reports a failure instead of a success", () => {
    showCodexUnifyMigrationFinished(
      { migratedStateRows: 0, migratedJsonlFiles: 0, error: "disk full" },
      t,
    );
    expect(errorMock).toHaveBeenCalledWith(
      "notifications.codexHistorySplit.migrationFailed",
      expect.anything(),
    );
    expect(successMock).not.toHaveBeenCalled();
  });
});
