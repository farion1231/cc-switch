import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const fetchExchangeRates = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({ usageApi: { fetchExchangeRates } }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));

import { useAutoExchangeRates } from "@/hooks/useAutoExchangeRates";

const fetched = {
  rates: { CNY: 6.9, EUR: 0.89 },
  fetchedAt: 1_700_000_000_000,
  source: "open.er-api.com",
};

describe("useAutoExchangeRates", () => {
  beforeEach(() => {
    fetchExchangeRates.mockReset();
  });

  it("fetches a stale automatic rate and saves it", async () => {
    fetchExchangeRates.mockResolvedValue(fetched);
    const save = vi.fn().mockResolvedValue(true);

    renderHook(() =>
      useAutoExchangeRates({ currency: "CNY", rateSource: "auto" }, save),
    );

    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        currency: "CNY",
        rateSource: "auto",
        autoExchangeRates: fetched.rates,
        rateFetchedAt: fetched.fetchedAt,
        rateProvider: fetched.source,
      }),
    );
    expect(fetchExchangeRates).toHaveBeenCalledTimes(1);
  });

  it("drops the fetched rate when the usage page was left meanwhile", async () => {
    let resolveFetch!: (value: typeof fetched) => void;
    fetchExchangeRates.mockReturnValueOnce(
      new Promise<typeof fetched>((resolve) => {
        resolveFetch = resolve;
      }),
    );
    const save = vi.fn().mockResolvedValue(true);

    const { unmount } = renderHook(() =>
      useAutoExchangeRates({ currency: "CNY", rateSource: "auto" }, save),
    );
    await waitFor(() => expect(fetchExchangeRates).toHaveBeenCalledTimes(1));
    // 请求在途时离开用量页：结果作废，不能用离开前的整份设置覆盖别处的修改
    unmount();
    await act(async () => {
      resolveFetch(fetched);
    });

    expect(save).not.toHaveBeenCalled();
  });

  it("does not touch the network for manual or fresh rates", () => {
    const save = vi.fn();
    const initial: { rateSource: string; fetchedAt: number | undefined } = {
      rateSource: "manual",
      fetchedAt: undefined,
    };
    const { rerender } = renderHook(
      ({ rateSource, fetchedAt }) =>
        useAutoExchangeRates(
          { currency: "CNY", rateSource, rateFetchedAt: fetchedAt },
          save,
        ),
      { initialProps: initial },
    );
    rerender({ rateSource: "auto", fetchedAt: Date.now() });

    expect(fetchExchangeRates).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
  });

  it("does not fetch while amounts are shown in USD", () => {
    renderHook(() =>
      useAutoExchangeRates({ currency: "USD", rateSource: "auto" }, vi.fn()),
    );

    expect(fetchExchangeRates).not.toHaveBeenCalled();
  });

  it("keeps the cached rate when the fetch fails", async () => {
    fetchExchangeRates.mockRejectedValue(new Error("offline"));
    const save = vi.fn();

    renderHook(() =>
      useAutoExchangeRates({ currency: "CNY", rateSource: "auto" }, save),
    );

    await waitFor(() => expect(fetchExchangeRates).toHaveBeenCalledTimes(1));
    expect(save).not.toHaveBeenCalled();
  });
});
