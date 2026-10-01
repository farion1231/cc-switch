import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import type { PresetModelSource } from "@/lib/modelMetadata";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
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

const renderFill = () => {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return renderHook(
    () =>
      useModelMetadataFill({
        baseUrl: "https://api.example.com/v1",
        presets,
        prefetch: false,
      }),
    { wrapper },
  );
};

describe("useModelMetadataFill", () => {
  it("applies preset values at once and models.dev values when they arrive", async () => {
    server.use(
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: {
              "model-x": { limit: { context: 1000000, output: 64000 } },
            },
          },
        }),
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
    server.use(http.get(MODELS_DEV_API_URL, () => HttpResponse.error()));
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("model-x", apply, "https://other.example.com/v1");
    expect(apply).not.toHaveBeenCalled();
  });

  it("keeps preset values and stays quiet when models.dev is unreachable", async () => {
    server.use(http.get(MODELS_DEV_API_URL, () => HttpResponse.error()));
    const { result } = renderFill();
    const apply = vi.fn(() => true);

    result.current("unknown", apply);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(apply).not.toHaveBeenCalled();
  });

  it("merges fills that changed something into one reminder", async () => {
    server.use(http.get(MODELS_DEV_API_URL, () => HttpResponse.error()));
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
});
