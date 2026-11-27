import { beforeEach, describe, expect, it, vi } from "vitest";

const warning = vi.fn();
const success = vi.fn();
const getModelsDevSyncConfig = vi.fn();
const saveModelsDevSyncConfig = vi.fn();

vi.mock("@/lib/toast", () => ({
  toast: { warning, success, error: vi.fn() },
}));
vi.mock("@/lib/api/usage", () => ({
  usageApi: { getModelsDevSyncConfig, saveModelsDevSyncConfig },
}));

async function load() {
  vi.resetModules();
  return import("@/lib/backgroundTaskNotice");
}

type Options = { action: { onClick: () => void }; description: string };
const lastOptions = () => warning.mock.calls.at(-1)?.[1] as Options;

describe("notifyBackgroundTaskFailed", () => {
  beforeEach(() => {
    warning.mockReset();
    success.mockReset();
    getModelsDevSyncConfig.mockReset();
    saveModelsDevSyncConfig.mockReset();
    localStorage.clear();
  });

  it("names the failed task and shows each task at most once per launch", async () => {
    const { notifyBackgroundTaskFailed } = await load();
    notifyBackgroundTaskFailed("updateCheck", new Error("offline"));
    notifyBackgroundTaskFailed("updateCheck", new Error("offline"));
    notifyBackgroundTaskFailed(
      "modelsDevSync",
      new Error(
        "error sending request for url (https://dl.ccswitch.io/latest.json)",
      ),
    );

    const { default: i18n } = await import("@/i18n");
    expect(warning.mock.calls.map((call) => call[0])).toEqual([
      i18n.t("backgroundTask.updateCheckFailed.title"),
      i18n.t("backgroundTask.modelsDevSyncFailed.title"),
    ]);
    // 连接层失败的原文换成一句话
    expect(lastOptions().description).toContain(
      i18n.t("backgroundTask.networkError"),
    );
    expect(lastOptions().description).not.toContain("offline");
  });

  it("stays quiet in later launches after the user mutes the update check", async () => {
    const first = await load();
    first.notifyBackgroundTaskFailed("updateCheck", "offline");
    lastOptions().action.onClick();

    const next = await load();
    next.notifyBackgroundTaskFailed("updateCheck", "offline");
    expect(warning).toHaveBeenCalledTimes(1);
  });

  it("turns off models.dev auto sync from the notice", async () => {
    const config = {
      autoSyncEnabled: true,
      includeCommonModels: true,
      selectedModelKeys: ["a"],
      excludedCommonModelKeys: [],
      lastSyncAt: null,
      lastSyncError: null,
    };
    getModelsDevSyncConfig.mockResolvedValue({ config, configPath: "/x" });
    saveModelsDevSyncConfig.mockResolvedValue(undefined);

    const { notifyBackgroundTaskFailed } = await load();
    notifyBackgroundTaskFailed("modelsDevSync", "offline");
    lastOptions().action.onClick();

    await vi.waitFor(() => expect(success).toHaveBeenCalled());
    expect(saveModelsDevSyncConfig).toHaveBeenCalledWith({
      ...config,
      autoSyncEnabled: false,
    });
  });
});
