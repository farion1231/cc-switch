import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useCallback } from "react";
import { useForm } from "react-hook-form";
import { useBaseUrlState } from "@/components/providers/forms/hooks";

/**
 * 复刻 ProviderForm 对 settingsConfig 的接线方式：
 * - settingsConfig 只用 form.setValue 写；这个 harness 不订阅它，写入不触发
 *   重渲染，用来稳定制造过期快照；
 * - 派生状态（这里是 Base URL）在渲染期用 form.getValues 取快照，那份快照因此
 *   会停在改动之前。
 *
 * 修法是给 hook 传 getSettingsConfig，让它在事件发生时现读当前值。不要指望
 * 靠重渲染刷新快照：真实表单里 Claude 分支已经订阅了 settingsConfig，照样会
 * 丢改动，事件处理里必须现读。
 *
 * 本测试锁住的不变量：后一次结构化字段改动不得覆盖前一次的改动。
 */
const INITIAL_CONFIG = JSON.stringify(
  { env: { ANTHROPIC_BASE_URL: "https://api.old.example" } },
  null,
  2,
);

const CONFIG_FROM_JSON_EDITOR = JSON.stringify(
  {
    env: {
      ANTHROPIC_BASE_URL: "https://api.old.example",
      ANTHROPIC_AUTH_TOKEN: "token-typed-in-json-editor",
    },
  },
  null,
  2,
);

function Harness({ onReadConfig }: { onReadConfig: (config: string) => void }) {
  const form = useForm<{ settingsConfig: string }>({
    defaultValues: { settingsConfig: INITIAL_CONFIG },
    mode: "onSubmit",
  });

  const handleSettingsConfigChange = useCallback(
    (config: string) => {
      if (form.getValues("settingsConfig") === config) {
        return;
      }
      form.setValue("settingsConfig", config);
    },
    [form],
  );

  const getSettingsConfig = useCallback(
    () => form.getValues("settingsConfig"),
    [form],
  );

  const { handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: "claude",
    category: "custom",
    settingsConfig: form.getValues("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: handleSettingsConfigChange,
    onCodexConfigChange: () => {},
    getSettingsConfig,
  });

  return (
    <div>
      <button
        type="button"
        onClick={() => handleSettingsConfigChange(CONFIG_FROM_JSON_EDITOR)}
      >
        edit-json
      </button>
      <button
        type="button"
        onClick={() => handleClaudeBaseUrlChange("https://api.new.example")}
      >
        edit-base-url
      </button>
      <button type="button" onClick={() => onReadConfig(getSettingsConfig())}>
        read
      </button>
    </div>
  );
}

// 不能从渲染结果里读：这个 harness 写 settingsConfig 不触发重渲染（用来制造
// 过期快照），渲染出来的文本会是旧值。改为按需从表单里现读。
let lastRead = "{}";
const renderHarness = () =>
  render(<Harness onReadConfig={(config) => (lastRead = config)} />);
const readConfig = () => {
  fireEvent.click(screen.getByText("read"));
  return JSON.parse(lastRead) as { env?: Record<string, string> };
};

describe("ProviderForm settingsConfig 写入不能丢失前一次改动", () => {
  it("JSON 编辑器改完再改请求地址，两处改动都要保留", () => {
    renderHarness();

    fireEvent.click(screen.getByText("edit-json"));
    expect(readConfig().env?.ANTHROPIC_AUTH_TOKEN).toBe(
      "token-typed-in-json-editor",
    );

    fireEvent.click(screen.getByText("edit-base-url"));

    const config = readConfig();
    expect(config.env?.ANTHROPIC_BASE_URL).toBe("https://api.new.example");
    expect(config.env?.ANTHROPIC_AUTH_TOKEN).toBe("token-typed-in-json-editor");
  });

  it("改请求地址后再改 JSON，请求地址不被旧快照覆盖", () => {
    renderHarness();

    fireEvent.click(screen.getByText("edit-base-url"));
    expect(readConfig().env?.ANTHROPIC_BASE_URL).toBe(
      "https://api.new.example",
    );

    fireEvent.click(screen.getByText("edit-json"));
    fireEvent.click(screen.getByText("edit-base-url"));

    const config = readConfig();
    expect(config.env?.ANTHROPIC_BASE_URL).toBe("https://api.new.example");
    expect(config.env?.ANTHROPIC_AUTH_TOKEN).toBe("token-typed-in-json-editor");
  });
});
