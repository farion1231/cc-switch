import { describe, expect, it } from "vitest";
import {
  huaweiCodeAgent3Preset,
  CODEAGENT_3_AUTH_HEADERS,
} from "./huaweiCodeAgent3Presets";

describe("huaweiCodeAgent3Preset", () => {
  it("has providerType huawei_codeagent_3", () => {
    expect(huaweiCodeAgent3Preset.providerType).toBe("huawei_codeagent_3");
  });

  it("uses maas-glm-5.1-zhipu as default model", () => {
    const env = (huaweiCodeAgent3Preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_MODEL).toBe("maas-glm-5.1-zhipu");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("maas-glm-5.1-zhipu");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("maas-glm-5.1-zhipu");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("maas-glm-5.1-zhipu");
  });

  it("has ANTHROPIC_BASE_URL for proxy server forwarding (removed from live config by TTY compat takeover)", () => {
    const env = (huaweiCodeAgent3Preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBeDefined();
    expect(env.ANTHROPIC_BASE_URL).toContain("snapengine.cida.cce");
  });

  it("does not route Claude Code traffic through a local connectivity proxy", () => {
    const env = (huaweiCodeAgent3Preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env).not.toHaveProperty("HTTPS_PROXY");
    expect(env).not.toHaveProperty("HTTP_PROXY");
    expect(env).not.toHaveProperty("NODE_TLS_REJECT_UNAUTHORIZED");
  });

  it("sets NO_PROXY for localhost", () => {
    const env = (huaweiCodeAgent3Preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env.NO_PROXY).toBe("127.0.0.1,localhost");
  });

  it("sets CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1", () => {
    const env = (huaweiCodeAgent3Preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC).toBe("1");
  });

  it("has name Huawei CodeAgent 3.0", () => {
    expect(huaweiCodeAgent3Preset.name).toBe("Huawei CodeAgent 3.0");
  });

  it("requires OAuth", () => {
    expect(huaweiCodeAgent3Preset.requiresOAuth).toBe(true);
  });

  it("has fingerprint replacements", () => {
    expect(huaweiCodeAgent3Preset.fingerprintReplacements).toBeDefined();
    expect(huaweiCodeAgent3Preset.fingerprintReplacements!.length).toBeGreaterThan(0);
  });
});

describe("CODEAGENT_3_AUTH_HEADERS", () => {
  it("includes Authorization header with valuePrefix Bearer", () => {
    const authHeader = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "Authorization");
    expect(authHeader).toBeDefined();
    expect(authHeader!.tokenSource).toBe("huawei_codeagent_2");
    expect(authHeader!.valuePrefix).toBe("Bearer ");
  });

  it("includes Model-Id header with tokenSource request_model", () => {
    const modelHeader = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "Model-Id");
    expect(modelHeader).toBeDefined();
    expect(modelHeader!.tokenSource).toBe("request_model");
  });

  it("includes x-auth-token header with tokenSource huawei_codeagent_2", () => {
    const tokenHeader = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "x-auth-token");
    expect(tokenHeader).toBeDefined();
    expect(tokenHeader!.tokenSource).toBe("huawei_codeagent_2");
  });

  it("includes Agent-Type header with value AgentCenter", () => {
    const header = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "Agent-Type");
    expect(header).toBeDefined();
    expect(header!.value).toBe("AgentCenter");
  });

  it("includes x-innercc-request-kind header with value chat", () => {
    const header = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "x-innercc-request-kind");
    expect(header).toBeDefined();
    expect(header!.value).toBe("chat");
  });

  it("includes x-error-propagation header with value true", () => {
    const header = CODEAGENT_3_AUTH_HEADERS.find((h) => h.name === "x-error-propagation");
    expect(header).toBeDefined();
    expect(header!.value).toBe("true");
  });

  it("does NOT include CodeAgent 2.0-specific headers", () => {
    const names = CODEAGENT_3_AUTH_HEADERS.map((h) => h.name);
    expect(names).not.toContain("oc-heartbeat");
    expect(names).not.toContain("queue");
    expect(names).not.toContain("x-session-id");
    expect(names).not.toContain("plugin-version");
    expect(names).not.toContain("depart");
  });

  it("has exactly 8 headers", () => {
    expect(CODEAGENT_3_AUTH_HEADERS).toHaveLength(8);
  });
});
