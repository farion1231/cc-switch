import { useCallback, useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { modelsDevQueryOptions, type ModelsDevResponse } from "@/lib/modelsDev";
import {
  resolveModelMetadata,
  type ModelMetadataSource,
  type PresetModelSource,
  type ResolvedModelMetadata,
} from "@/lib/modelMetadata";
import { useLatestRef } from "./useLatestRef";

const FILLED_TOAST_ID = "model-metadata-filled";

/**
 * 表单从拉取列表里选中模型后，调 `fill(modelId, apply)` 补这一行空着的参数。
 *
 * `apply(metadata)` 把参数补进当时仍是这个模型的那一行并提交，返回是否真的改动
 * 了字段。它会被调用最多两次：当场一次（同地址预设 + 已缓存的 models.dev），
 * models.dev 数据晚到时再一次，所以调用方先提交「改模型 ID」，`apply` 读最新的
 * 行、确认模型没被用户改掉、只补空字段。有改动时由这里提示用户核对；同一轮里
 * 补了几个模型（批量添加、两次先后到达）只合并成一条提示。
 *
 * `prefetch` 为真（已经拉过模型列表）时提前下载 models.dev，和价格导入共用同
 * 一份查询缓存；离线或下载失败时只用预设里的值，不打扰用户。
 */
export function useModelMetadataFill({
  baseUrl,
  presets,
  prefetch,
}: {
  baseUrl: string;
  presets: () => readonly PresetModelSource[];
  prefetch: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  // 只下载不订阅：表单不读这份数据，数据到了也不必重渲染。
  useEffect(() => {
    if (prefetch) void queryClient.prefetchQuery(modelsDevQueryOptions);
  }, [prefetch, queryClient]);

  const baseUrlRef = useLatestRef(baseUrl);
  const presetsRef = useLatestRef(presets);

  const pending = useRef<{
    models: Set<string>;
    sources: Set<ModelMetadataSource>;
    timer?: ReturnType<typeof setTimeout>;
  }>({ models: new Set(), sources: new Set() });
  useEffect(() => () => clearTimeout(pending.current.timer), []);

  const notify = useCallback(
    (modelId: string, sources: readonly ModelMetadataSource[]) => {
      const batch = pending.current;
      batch.models.add(modelId);
      for (const source of sources) batch.sources.add(source);
      clearTimeout(batch.timer);
      batch.timer = setTimeout(() => {
        const models = [...batch.models];
        const fromPreset = batch.sources.has("preset");
        const fromModelsDev = batch.sources.has("models-dev");
        batch.models.clear();
        batch.sources.clear();
        const source =
          fromPreset && fromModelsDev
            ? t("providerForm.modelMetadataSourceBoth", {
                defaultValue: "预设和 models.dev",
              })
            : fromPreset
              ? t("providerForm.modelMetadataSourcePreset", {
                  defaultValue: "预设",
                })
              : "models.dev";
        toast.info(
          models.length === 1
            ? t("providerForm.modelMetadataFilled", {
                model: models[0],
                source,
                defaultValue:
                  "已补全 {{model}} 的上下文等参数（来源：{{source}}），请核对",
              })
            : t("providerForm.modelMetadataFilledMany", {
                count: models.length,
                source,
                defaultValue:
                  "已补全 {{count}} 个模型的上下文等参数（来源：{{source}}），请核对",
              }),
          { id: FILLED_TOAST_ID },
        );
      }, 50);
    },
    [t],
  );

  return useCallback(
    (
      modelId: string,
      apply: (metadata: ResolvedModelMetadata) => boolean,
      /** 这一行实际请求的地址（模型级覆盖时传入），默认用表单的地址。 */
      baseUrl?: string,
    ): void => {
      const resolve = (modelsDev?: ModelsDevResponse) =>
        resolveModelMetadata(modelId, {
          baseUrl: baseUrl || baseUrlRef.current,
          presets: presetsRef.current(),
          modelsDev,
        });
      const run = (metadata: ResolvedModelMetadata | null) => {
        if (metadata && apply(metadata)) notify(modelId, metadata.sources);
      };

      const cached = queryClient.getQueryData(modelsDevQueryOptions.queryKey);
      run(resolve(cached));
      if (cached) return;
      queryClient
        .fetchQuery(modelsDevQueryOptions)
        .then((data) => run(resolve(data)))
        .catch(() => {
          // 离线或 models.dev 不可用：保持预设值补过的样子。
        });
    },
    [baseUrlRef, notify, presetsRef, queryClient],
  );
}
