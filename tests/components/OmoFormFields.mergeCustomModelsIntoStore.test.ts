import { describe, expect, it } from "vitest";
import {
  mergeCustomModelsIntoStore,
  type CustomModelItem,
} from "@/components/providers/forms/OmoFormFields";

describe("mergeCustomModelsIntoStore", () => {
  it("保留自定义项高级字段，并在模型变更时仅按需清理非法 variant", () => {
    const store = {
      sisyphus: { model: "builtin-model" },
      "custom-agent": {
        model: "model-a",
        variant: "fast",
        temperature: 0.2,
        permission: { edit: "allow" },
      },
    };
    const customs: CustomModelItem[] = [
      { key: "custom-agent", model: "model-b", sourceKey: "custom-agent" },
    ];

    const merged = mergeCustomModelsIntoStore(
      store,
      new Set(["sisyphus"]),
      customs,
      { "model-b": ["precise"] },
    );

    expect(merged.sisyphus).toEqual({ model: "builtin-model" });
    expect(merged["custom-agent"]).toEqual({
      model: "model-b",
      temperature: 0.2,
      permission: { edit: "allow" },
    });
  });

  it("重命名自定义 key 时迁移原有 variant 和高级字段", () => {
    const store = {
      sisyphus: { model: "builtin-model" },
      "custom-agent-old": {
        model: "model-a",
        variant: "fast",
        maxTokens: 8192,
      },
    };
    const customs: CustomModelItem[] = [
      {
        key: "custom-agent-new",
        sourceKey: "custom-agent-old",
        model: "model-a",
      },
    ];

    const merged = mergeCustomModelsIntoStore(
      store,
      new Set(["sisyphus"]),
      customs,
      { "model-a": ["fast", "balanced"] },
    );

    expect(merged["custom-agent-old"]).toBeUndefined();
    expect(merged["custom-agent-new"]).toEqual({
      model: "model-a",
      variant: "fast",
      maxTokens: 8192,
    });
  });

  it.each(["rename another", "delete another", "rename current"])(
    "preserves an unavailable saved variant when %s custom entry",
    (operation) => {
      const saved = {
        model: "provider/model",
        variant: "medium",
        temperature: 0.2,
      };
      const store = {
        first: saved,
        second: { model: "provider/model" },
      };
      const targetKey = operation === "rename current" ? "renamed" : "first";
      const customs = [
        { key: targetKey, sourceKey: "first", model: "provider/model" },
        ...(operation === "delete another"
          ? []
          : [{ key: "other", sourceKey: "second", model: "provider/model" }]),
      ];

      const merged = mergeCustomModelsIntoStore(store, new Set(), customs, {
        "provider/model": ["low", "high"],
      });

      expect(merged[targetKey]).toEqual(saved);
      expect(store.first).toEqual(saved);
    },
  );

  it("keeps a saved variant supported by an explicitly selected new model", () => {
    const merged = mergeCustomModelsIntoStore(
      { first: { model: "model-a", variant: "medium", temperature: 0.2 } },
      new Set(),
      [{ key: "first", sourceKey: "first", model: "model-b" }],
      { "model-b": ["medium"] },
    );
    expect(merged.first).toEqual({
      model: "model-b",
      variant: "medium",
      temperature: 0.2,
    });
  });

  it("custom 列表为空时移除旧自定义项但保留内置项", () => {
    const store = {
      sisyphus: { model: "builtin-model" },
      hephaestus: { model: "builtin-model-2" },
      "custom-agent": { model: "model-a", temperature: 0.3 },
    };

    const merged = mergeCustomModelsIntoStore(
      store,
      new Set(["sisyphus", "hephaestus"]),
      [],
      {},
    );

    expect(merged).toEqual({
      sisyphus: { model: "builtin-model" },
      hephaestus: { model: "builtin-model-2" },
    });
  });

  it("清空 model 时保留高级字段并移除 model/variant", () => {
    const store = {
      sisyphus: { model: "builtin-model" },
      "custom-agent": {
        model: "model-a",
        variant: "fast",
        temperature: 0.7,
      },
    };
    const customs: CustomModelItem[] = [
      { key: "custom-agent", model: "", sourceKey: "custom-agent" },
    ];

    const merged = mergeCustomModelsIntoStore(
      store,
      new Set(["sisyphus"]),
      customs,
      { "model-a": ["fast"] },
    );

    expect(merged["custom-agent"]).toEqual({ temperature: 0.7 });
  });
});
