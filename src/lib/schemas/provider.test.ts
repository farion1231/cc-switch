import { describe, expect, it } from "vitest";

import { providerSchema } from "./provider";

describe("providerSchema iconSource (issue #8117)", () => {
  it("accepts legacy records without iconSource", () => {
    const result = providerSchema.safeParse({
      name: "Legacy",
      settingsConfig: "{}",
      icon: "openai",
      iconColor: "#00A67E",
    });
    expect(result.success).toBe(true);
  });

  it("accepts builtin source with an icon name", () => {
    const result = providerSchema.safeParse({
      name: "Builtin",
      settingsConfig: "{}",
      icon: "openai",
      iconSource: "builtin",
    });
    expect(result.success).toBe(true);
  });

  it("accepts local source with a well-formed asset key", () => {
    const result = providerSchema.safeParse({
      name: "Local",
      settingsConfig: "{}",
      icon: "myprovider-ab12cd34.png",
      iconSource: "local",
    });
    expect(result.success).toBe(true);
  });

  it("rejects local source with a non-asset-key icon", () => {
    const result = providerSchema.safeParse({
      name: "Bad local",
      settingsConfig: "{}",
      icon: "openai",
      iconSource: "local",
    });
    expect(result.success).toBe(false);
  });

  it("accepts url source with an http(s) image URL", () => {
    const result = providerSchema.safeParse({
      name: "Url",
      settingsConfig: "{}",
      icon: "https://example.com/icon.png",
      iconSource: "url",
    });
    expect(result.success).toBe(true);
  });

  it("rejects url source with a non-URL icon", () => {
    const result = providerSchema.safeParse({
      name: "Bad url",
      settingsConfig: "{}",
      icon: "openai",
      iconSource: "url",
    });
    expect(result.success).toBe(false);
  });

  it("rejects unknown iconSource values", () => {
    const result = providerSchema.safeParse({
      name: "Bad source",
      settingsConfig: "{}",
      icon: "openai",
      iconSource: "base64",
    });
    expect(result.success).toBe(false);
  });
});
