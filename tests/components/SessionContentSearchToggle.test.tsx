import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";
import { providersApi, sessionsApi, settingsApi } from "@/lib/api";
import type { Settings } from "@/types";
import { setSessionFixtures, setSettings } from "../msw/state";

const toastError = vi.hoisted(() => vi.fn());
vi.mock("@/lib/toast", () => ({
  toast: { error: toastError, success: vi.fn() },
}));

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

const openToggle = async () => {
  await userEvent.click(screen.getByRole("button", { name: "会话的更多操作" }));
  return screen.findByRole("menuitemcheckbox", { name: "搜索消息正文" });
};

const setup = async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const page = () => (
    <QueryClientProvider client={client}>
      <SessionManagerPage appId="codex" />
    </QueryClientProvider>
  );
  const view = render(page());
  await waitFor(() =>
    expect(client.getQueryData<Settings>(["settings"])).toBeDefined(),
  );
  await waitFor(() => expect(sessionsApi.syncContentIndex).toHaveBeenCalled());
  return { client, view, page };
};

const persistedEnabled = (client: QueryClient) =>
  client.getQueryData<Settings>(["settings"])?.sessionContentSearchEnabled;

// Keep the real component, settings hook, mutation and query invalidation. Only
// the API boundary is controlled; no real sessions or on-disk index are touched.
describe("session content search toggle ordering", () => {
  let indexExists: boolean;
  beforeEach(() => {
    setSettings({ sessionContentSearchEnabled: true });
    setSessionFixtures(
      [
        {
          providerId: "codex",
          sessionId: "fixture",
          title: "Fixture",
          sourcePath: "/mock/codex/fixture.jsonl",
        },
      ],
      {},
    );
    indexExists = false;
    vi.spyOn(providersApi, "updateTrayMenu").mockResolvedValue(true);
    vi.spyOn(sessionsApi, "syncContentIndex").mockImplementation(async () => {
      indexExists = true;
      return { running: false, processed: 1, total: 1 };
    });
    vi.spyOn(sessionsApi, "clearContentIndex").mockImplementation(async () => {
      indexExists = false;
      return true;
    });
    vi.spyOn(settingsApi, "save");
  });
  afterEach(() => vi.restoreAllMocks());

  it("finishes disabling and clearing before accepting another enable", async () => {
    const tray = deferred<boolean>();
    vi.mocked(providersApi.updateTrayMenu).mockImplementationOnce(
      () => tray.promise,
    );
    const { client } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() => expect(persistedEnabled(client)).toBe(false));
    expect(sessionsApi.clearContentIndex).not.toHaveBeenCalled();

    // Settings have already refreshed, but autoSaveSettings is still awaiting
    // the tray. This is the real window in which the old disable used to race.
    const toggle = await openToggle();
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    await act(async () => {
      await Promise.resolve();
    });
    expect(settingsApi.save).toHaveBeenCalledTimes(1);
    expect(toggle).toHaveAttribute("aria-disabled", "true");

    await act(async () => {
      tray.resolve(true);
    });
    await waitFor(() =>
      expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(1),
    );
    expect(indexExists).toBe(false);
    await waitFor(() =>
      expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
    );
    await userEvent.click(toggle);
    await waitFor(() => expect(persistedEnabled(client)).toBe(true));
    await waitFor(() => expect(indexExists).toBe(true));
    expect(settingsApi.save).toHaveBeenCalledTimes(2);
    expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(1);
  });

  it("does not start a second save during repeated clicks while persistence is pending", async () => {
    const saving = deferred<boolean>();
    const save = vi.mocked(settingsApi.save);
    vi.mocked(settingsApi.save).mockImplementationOnce(async (settings) => {
      await saving.promise;
      setSettings(settings);
      return true;
    });
    const { client } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    const toggle = await openToggle();
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    expect(save).toHaveBeenCalledTimes(1);
    expect(persistedEnabled(client)).toBe(true);
    expect(sessionsApi.clearContentIndex).not.toHaveBeenCalled();
    await act(async () => {
      saving.resolve(true);
    });
    await waitFor(() =>
      expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
    );
    expect(persistedEnabled(client)).toBe(false);
    expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(1);
    expect(indexExists).toBe(false);
  });

  it("clears on the final disable in a completed disable-enable-disable cycle", async () => {
    const { client } = await setup();
    for (const enabled of [false, true, false]) {
      const toggle = await openToggle();
      await waitFor(() =>
        expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
      );
      await userEvent.click(toggle);
      await waitFor(() => expect(persistedEnabled(client)).toBe(enabled));
      await waitFor(() => expect(client.isMutating()).toBe(0));
      expect(indexExists).toBe(enabled);
    }
    expect(settingsApi.save).toHaveBeenCalledTimes(3);
    expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(2);
  });

  it("also keeps the toggle busy until asynchronous clearing finishes", async () => {
    const clear = deferred<boolean>();
    vi.mocked(sessionsApi.clearContentIndex).mockImplementationOnce(
      async () => {
        const result = await clear.promise;
        indexExists = false;
        return result;
      },
    );
    const { client } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() =>
      expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(1),
    );
    const toggle = await openToggle();
    fireEvent.click(toggle);
    expect(settingsApi.save).toHaveBeenCalledTimes(1);
    expect(persistedEnabled(client)).toBe(false);
    await act(async () => {
      clear.resolve(true);
    });
    await waitFor(() =>
      expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
    );
    expect(indexExists).toBe(false);
    await userEvent.click(toggle);
    await waitFor(() => expect(persistedEnabled(client)).toBe(true));
    await waitFor(() => expect(indexExists).toBe(true));
  });

  it("keeps an in-flight disable locked across page remounts", async () => {
    const tray = deferred<boolean>();
    vi.mocked(providersApi.updateTrayMenu).mockImplementationOnce(
      () => tray.promise,
    );
    const { client, view, page } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() => expect(persistedEnabled(client)).toBe(false));
    view.unmount();
    render(page());
    const toggle = await openToggle();
    fireEvent.click(toggle);
    expect(settingsApi.save).toHaveBeenCalledTimes(1);
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    await act(async () => {
      tray.resolve(true);
    });
    await waitFor(() =>
      expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
    );
    expect(indexExists).toBe(false);
    await userEvent.click(toggle);
    await waitFor(() => expect(persistedEnabled(client)).toBe(true));
    await waitFor(() => expect(indexExists).toBe(true));
  });

  it("does not clear after a failed save and permits a successful retry", async () => {
    vi.mocked(settingsApi.save).mockRejectedValueOnce(new Error("save failed"));
    const { client } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        expect.stringContaining("save failed"),
      ),
    );
    expect(persistedEnabled(client)).toBe(true);
    expect(indexExists).toBe(true);
    expect(sessionsApi.clearContentIndex).not.toHaveBeenCalled();
    await userEvent.click(await openToggle());
    await waitFor(() =>
      expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(1),
    );
    expect(persistedEnabled(client)).toBe(false);
    expect(indexExists).toBe(false);
  });

  it("reports clear failures and releases the toggle for enable and disable again", async () => {
    vi.mocked(sessionsApi.clearContentIndex).mockRejectedValueOnce(
      new Error("clear failed"),
    );
    const { client } = await setup();
    await userEvent.click(await openToggle());
    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(
        expect.stringContaining("clear failed"),
      ),
    );
    expect(persistedEnabled(client)).toBe(false);
    expect(indexExists).toBe(true);
    await userEvent.click(await openToggle());
    await waitFor(() => expect(persistedEnabled(client)).toBe(true));
    const toggle = await openToggle();
    await waitFor(() =>
      expect(toggle).not.toHaveAttribute("aria-disabled", "true"),
    );
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(sessionsApi.clearContentIndex).toHaveBeenCalledTimes(2),
    );
    expect(persistedEnabled(client)).toBe(false);
    expect(indexExists).toBe(false);
  });
});
