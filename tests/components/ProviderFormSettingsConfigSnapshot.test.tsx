import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useCallback, useState } from "react";
import { useForm } from "react-hook-form";
import { useBaseUrlState } from "@/components/providers/forms/hooks";

/**
 * 复刻 ProviderForm 对 settingsConfig 的接线方式：
 * - settingsConfig 没有通过 register/Controller 订阅，只能用 form.setValue 写；
 * - 派生状态（这里是 Base URL）在渲染期用 form.getValues 取快照。
 *
 * 少了写入时的强制重渲染，那份快照会停在改动之前，下一个结构化字段一动就把
 * JSON 编辑器里刚输入的内容按旧快照覆盖回去 —— 表现为"改了配置、保存成功、
 * 文件里还是旧值"。本测试锁住这个不变量。
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

function Harness() {
  const form = useForm<{ settingsConfig: string }>({
    defaultValues: { settingsConfig: INITIAL_CONFIG },
    mode: "onSubmit",
  });

  const [, bumpSettingsConfigRevision] = useState(0);
  const handleSettingsConfigChange = useCallback(
    (config: string) => {
      if (form.getValues("settingsConfig") === config) {
        return;
      }
      form.setValue("settingsConfig", config);
      bumpSettingsConfigRevision((revision) => revision + 1);
    },
    [form],
  );

  const { handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: "claude",
    category: "custom",
    settingsConfig: form.getValues("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: handleSettingsConfigChange,
    onCodexConfigChange: () => {},
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
      <output data-testid="settings-config">
        {form.getValues("settingsConfig")}
      </output>
    </div>
  );
}

const readConfig = () =>
  JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}") as {
    env?: Record<string, string>;
  };

describe("ProviderForm settingsConfig 写入不能丢失前一次改动", () => {
  it("JSON 编辑器改完再改请求地址，两处改动都要保留", () => {
    render(<Harness />);

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
    render(<Harness />);

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
