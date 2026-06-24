import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  formatKimiSettingsForEditor,
  serializeKimiSettingsForBackend,
} from "@/utils/kimiConfigUtils";

describe("kimiConfigUtils", () => {
  it("expands legacy TOML string config for JSON editor display", () => {
    const editorValue = formatKimiSettingsForEditor(
      JSON.stringify({
        config:
          'default_model = "kimi-code/kimi-for-coding"\n\n[providers."managed:kimi-code"]\ntype = "kimi"\napi_key = ""\nbase_url = "https://api.kimi.com/coding/v1"\n\n[models."kimi-code/kimi-for-coding"]\nprovider = "managed:kimi-code"\nmodel = "kimi-for-coding"\nmax_context_size = 262144\n',
      }),
    );

    const parsed = JSON.parse(editorValue);
    expect(parsed.config.providers["managed:kimi-code"].base_url).toBe(
      "https://api.kimi.com/coding/v1",
    );
    expect(
      parsed.config.models["kimi-code/kimi-for-coding"].max_context_size,
    ).toBe(262144);
  });

  it("serializes editor JSON config object back to backend TOML string", () => {
    const backendValue = serializeKimiSettingsForBackend(
      JSON.stringify({
        config: {
          default_model: "kimi-code/kimi-for-coding",
          providers: {
            "managed:kimi-code": {
              type: "kimi",
              api_key: "",
              base_url: "https://api.kimi.com/coding/v1",
              custom_headers: {},
            },
          },
          models: {
            "kimi-code/kimi-for-coding": {
              provider: "managed:kimi-code",
              model: "kimi-for-coding",
              max_context_size: 262144,
              capabilities: ["thinking", "tool_use"],
            },
          },
          thinking: {
            mode: "auto",
          },
        },
      }),
    );

    const parsed = JSON.parse(backendValue);
    expect(typeof parsed.config).toBe("string");
    expect(parsed.config).toContain('[providers."managed:kimi-code"]');
    expect(parsed.config).not.toContain("custom_headers");

    const toml = parseToml(parsed.config);
    expect((toml.providers as any)["managed:kimi-code"].base_url).toBe(
      "https://api.kimi.com/coding/v1",
    );
    expect(
      (toml.models as any)["kimi-code/kimi-for-coding"].capabilities,
    ).toEqual(["thinking", "tool_use"]);
  });
});
