import { describe, expect, it } from "vitest";
import { TEMPLATE_TYPES } from "@/config/constants";
import { generatePresetTemplates } from "@/components/UsageScriptModal";

const mockT = (key: string) => key;

describe("UsageScriptModal preset templates", () => {
  it("uses the token usage endpoint for the New API template", () => {
    const templates = generatePresetTemplates(mockT);
    const newApiTemplate = templates[TEMPLATE_TYPES.NEW_API];

    expect(newApiTemplate).toContain("{{baseUrl}}/api/usage/token");
    expect(newApiTemplate).toContain('method: "GET"');
    expect(newApiTemplate).toContain('"Authorization": "Bearer {{apiKey}}"');
    expect(newApiTemplate).toContain('"User-Agent": "cc-switch/1.0"');
    expect(newApiTemplate).toContain("response.code === true");
    expect(newApiTemplate).toContain(
      "response?.data?.total_available !== undefined",
    );
    expect(newApiTemplate).toContain("response.data.total_available / 500000");
    expect(newApiTemplate).not.toContain("/api/user/self");
    expect(newApiTemplate).not.toContain("New-Api-User");
    expect(newApiTemplate).not.toContain("Content-Type");
    expect(newApiTemplate).not.toContain("{{accessToken}}");
    expect(newApiTemplate).not.toContain("{{userId}}");
  });
});
