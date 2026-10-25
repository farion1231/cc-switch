import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// These integration surfaces must dispatch through extensions. Vendor behavior
// belongs in src/huawei and src-tauri/src/huawei, including regression fixtures.
describe("CodeAgent module boundary", () => {
  it.each([
    "src/components/providers/forms/ProviderForm.tsx",
    "src/components/providers/forms/ClaudeDesktopProviderForm.tsx",
    "src-tauri/src/claude_desktop_config.rs",
    "src-tauri/src/proxy/handlers.rs",
    "src-tauri/src/proxy/forwarder.rs",
    "src-tauri/src/proxy/response_processor.rs",
    "src-tauri/src/proxy/http_client.rs",
    "src-tauri/src/proxy/hooks.rs",
    "src-tauri/src/proxy/providers/claude.rs",
    "src-tauri/src/proxy/providers/auth.rs",
  ])("%s has no Huawei-specific production branches", (file) => {
    const source = readFileSync(file, "utf8").split("#[cfg(test)]")[0];
    expect(source).not.toMatch(
      /@\/huawei|crate::huawei|super::huawei|with_proxy_hooks|isHuaweiProviderType|huawei_codeagent_|huawei_w3|huawei_idea|HuaweiCodeAgent|\.huawei\.com|headerMode|skipTlsVerify/,
    );
  });
});
