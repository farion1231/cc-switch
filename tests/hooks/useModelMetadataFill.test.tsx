import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import type { PresetModelSource } from "@/lib/modelMetadata";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastInfo = vi.hoisted(() => vi.fn());
vi.mock("sonner", () => ({ toast: { info: toastInfo } }));

const presets = (): PresetModelSource[] => [
  {
    baseUrl: "https://api.example.com/v1",
    models: new Map([
      ["model-x", { contextWindow: 500000 }],
      ["model-y", { contextWindow: 1000 }],
    ]),
  },
];

const renderFill = (
  options: {
    baseUrl?: string;
    fetchedModels?: Parameters<typeof useModelMetadataFill>[0]["fetchedModels"];
    fetchedFrom?: string;
  } = {},
) => {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return renderHook(
    () =>
      useModelMetadataFill({
        baseUrl: options.baseUrl ?? "https://api.example.com/v1",
        presets,
        prefetch: false,
        fetchedModels: options.fetchedModels,
        fetchedFrom: options.fetchedFrom,
      }),
    { wrapper },
  );
};

describe("useModelMetadataFill", () => {
  it("applies preset values at once and models.dev values when they arrive", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.json(
          JSON.stringify({
            example: {
              api: "https://api.example.com/v1",
              models: {
                "model-x": { limit: { context: 1000000, output: 64000 } },
              },
            },
          }),
        ),
      ),
    );
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("model-x", apply);

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith({
      contextWindow: 500000,
      sources: ["preset"],
    });
    await waitFor(() =>
      expect(apply).toHaveBeenLastCalledWith({
        contextWindow: 500000,
        maxOutputTokens: 64000,
        sources: ["preset", "models-dev"],
      }),
    );
  });

  it("looks a row up by its own address when it overrides the form's", () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.error(),
      ),
    );
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("model-x", apply, {
      baseUrl: "https://other.example.com/v1",
    });
    expect(apply).not.toHaveBeenCalled();
  });

  it("uses gateway metadata only when the list came from the same endpoint", () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.error(),
      ),
    );
    const gatewayModels = [
      {
        id: "model-x",
        ownedBy: null,
        metadata: { meta: { models_dev: { limit: { context: 123456 } } } },
      },
    ];
    const applyFrom = (baseUrl: string, fetchedFrom: string) =>
      renderFill({ baseUrl, fetchedModels: gatewayModels, fetchedFrom });

    const matching = applyFrom(
      "https://api.example.com/v1",
      "https://api.example.com/v1",
    );
    const matchingApply = vi.fn(() => true);
    matching.result.current("model-x", matchingApply);
    expect(matchingApply).toHaveBeenCalledWith({
      contextWindow: 123456,
      sources: ["gateway"],
    });

    const mismatched = applyFrom(
      "https://api.example.com/v1",
      "https://api.example.com/v1",
    );
    const mismatchedApply = vi.fn(() => true);
    // 行单独指向别的地址时，不使用供应商级列表的元数据。
    mismatched.result.current("model-x", mismatchedApply, {
      baseUrl: "https://other.example.com/v1",
    });
    expect(mismatchedApply).not.toHaveBeenCalled();
  });

  it("keeps preset values and stays quiet when models.dev is unreachable", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.error(),
      ),
    );
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("unknown", apply);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(apply).not.toHaveBeenCalled();
  });

  it("merges fills that changed something into one reminder", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.error(),
      ),
    );
    toastInfo.mockClear();
    const { result } = renderFill();

    result.current("model-x", () => true);
    result.current("model-y", () => true);
    result.current("model-x", () => false);
    await waitFor(() => expect(toastInfo).toHaveBeenCalledTimes(1));
    expect(toastInfo).toHaveBeenCalledWith(
      "已补全 2 个模型的上下文等参数（来源：预设），请核对",
      { id: "model-metadata-filled" },
    );

    toastInfo.mockClear();
    result.current("model-y", () => false);
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(toastInfo).not.toHaveBeenCalled();
  });
  it("fills without a reminder when asked to stay silent", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.error(),
      ),
    );
    toastInfo.mockClear();
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("model-x", apply, { silent: true });
    expect(apply).toHaveBeenCalledTimes(1);
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(toastInfo).not.toHaveBeenCalled();
  });
});
