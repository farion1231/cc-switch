import { describe, it, expect } from "vitest";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import { settingsSchema, type SettingsFormData } from "@/lib/schemas/settings";
import { mcpServerSchema, type McpServerFormData } from "@/lib/schemas/mcp";
import { jsonConfigSchema, tomlConfigSchema } from "@/lib/schemas/common";

describe("parseJsonError (provider.ts)", () => {
  describe("Chrome/V8 error format", () => {
    it("should parse Chrome/V8 'at position' format with providerSchema", () => {
      const data: ProviderFormData = {
        name: "Test Provider",
        settingsConfig: '{"invalid": }', // Invalid JSON
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        const settingsError = result.error.issues.find((issue) =>
          issue.path.includes("settingsConfig")
        );
        expect(settingsError?.message).toBeDefined();
      }
    });

    it("should handle Chrome 'Unexpected token' at various positions", () => {
      const testCases = [
        '{"key": undefined}',
        '{"key": NaN}',
        "{'key': 'value'}",
      ];

      for (const testCase of testCases) {
        expect(() => JSON.parse(testCase)).toThrow();
      }
    });
  });

  describe("Firefox error format", () => {
    it("should parse Firefox 'line X column Y' format with providerSchema", () => {
      const data: ProviderFormData = {
        name: "Test Provider",
        settingsConfig: '{"key":\n\ninvalid}', // Invalid JSON
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });
  });

  describe("Generic browser error format", () => {
    it("should handle generic JSON parse errors with providerSchema", () => {
      const data: ProviderFormData = {
        name: "Test Provider",
        settingsConfig: "[1, 2, 3,]", // Invalid JSON (trailing comma)
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        const settingsError = result.error.issues.find((issue) =>
          issue.path.includes("settingsConfig")
        );
        expect(settingsError?.message).toContain("JSON");
      }
    });
  });
});

describe("providerSchema", () => {
  describe("valid data", () => {
    it("should accept valid minimal provider", () => {
      const data: ProviderFormData = {
        name: "My Provider",
        settingsConfig: '{"apiKey": "test"}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept provider with all fields", () => {
      const data: ProviderFormData = {
        name: "Full Provider",
        websiteUrl: "https://example.com",
        notes: "Test notes",
        settingsConfig: '{"key": "value", "nested": {"field": 123}}',
        icon: "🔑",
        iconColor: "#FF5733",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept empty string for optional websiteUrl", () => {
      const data: ProviderFormData = {
        name: "Provider",
        websiteUrl: "",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept provider with complex nested JSON config", () => {
      const data: ProviderFormData = {
        name: "Complex Provider",
        settingsConfig: JSON.stringify({
          env: { ANTHROPIC_AUTH_TOKEN: "test-token" },
          mcpServers: {
            "mcp-fetch": {
              command: "npx",
              args: ["@modelcontextprotocol/server-fetch"],
            },
          },
        }),
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept provider with unicode characters in name and notes", () => {
      const data: ProviderFormData = {
        name: "提供商 🚀 プロバイダー",
        notes: "测试笔记 🎯 テストノート",
        settingsConfig: '{"field": "值"}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });

  describe("invalid data", () => {
    it("should reject provider with invalid websiteUrl", () => {
      const data: ProviderFormData = {
        name: "Provider",
        websiteUrl: "not-a-valid-url",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].path).toContain("websiteUrl");
      }
    });

    it("should reject provider with invalid JSON config", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "{invalid json}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].path).toContain("settingsConfig");
        expect(result.error.issues[0].message).toContain("JSON");
      }
    });

    it("should reject provider with empty settingsConfig", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].path).toContain("settingsConfig");
      }
    });

    it("should reject provider with whitespace-only settingsConfig", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "   ",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject provider with incomplete JSON", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"key": "value"',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject provider with trailing commas in JSON", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"key": "value",}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject provider with unquoted keys in JSON", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "{key: 'value'}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject provider with single quotes in JSON", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "{'key': 'value'}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });
  });

  describe("edge cases", () => {
    it("should handle JSON with null values", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"key": null}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle JSON with boolean values", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"enabled": true, "disabled": false}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle JSON with numeric values", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"timeout": 30000, "retries": 3, "rate": 0.95}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle JSON with escape sequences", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig:
          '{"path": "C:\\\\Users\\\\test", "quote": "\\"quoted\\""}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle JSON with unicode escape sequences", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig:
          '{"emoji": "\\ud83d\\ude00", "chinese": "\\u4e2d\\u6587"}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle JSON with special HTML characters", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"html": "<script>alert(1)</script>"}',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle very large JSON structures", () => {
      const largeConfig = {
        items: Array.from({ length: 1000 }, (_, i) => ({
          id: i,
          value: `item-${i}`,
        })),
      };

      const data: ProviderFormData = {
        name: "Large Provider",
        settingsConfig: JSON.stringify(largeConfig),
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle deeply nested JSON structures", () => {
      let nested: any = { value: "deep" };
      for (let i = 0; i < 50; i++) {
        nested = { level: nested };
      }

      const data: ProviderFormData = {
        name: "Deep Provider",
        settingsConfig: JSON.stringify(nested),
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle name with special characters", () => {
      const data: ProviderFormData = {
        name: "Provider@#$%^&*()_+-=[]{}|;:',.<>?/`~",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle name with newlines and tabs", () => {
      const data: ProviderFormData = {
        name: "Provider\nWith\nNewlines\tAnd\tTabs",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle URL with special characters and query parameters", () => {
      const data: ProviderFormData = {
        name: "Provider",
        websiteUrl: "https://example.com/path?key=value&other=123#anchor",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle icon as emoji and special Unicode", () => {
      const data: ProviderFormData = {
        name: "Provider",
        icon: "🔐🚀💻🌟",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should handle very long icon color code", () => {
      const data: ProviderFormData = {
        name: "Provider",
        iconColor: "#FFFFFF",
        settingsConfig: "{}",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });

  describe("JSON error message extraction", () => {
    it("should extract error position from Chrome errors", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: '{"incomplete": ',
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        const message = result.error.issues[0].message;
        expect(message).toContain("JSON");
      }
    });

    it("should handle null JSON value", () => {
      const data: ProviderFormData = {
        name: "Provider",
        settingsConfig: "null",
      };

      const result = providerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });
});

describe("settingsSchema", () => {
  describe("valid data", () => {
    it("should accept minimal valid settings", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept all UI settings", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: true,
        enableClaudePluginIntegration: true,
        skipClaudeOnboarding: false,
        launchOnStartup: true,
        enableLocalProxy: false,
        language: "en",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept all languages", () => {
      for (const lang of ["en", "zh", "ja"]) {
        const data: SettingsFormData = {
          showInTray: true,
          minimizeToTrayOnClose: false,
          language: lang as "en" | "zh" | "ja",
        };

        const result = settingsSchema.safeParse(data);
        expect(result.success).toBe(true);
      }
    });

    it("should accept directory paths", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "/home/user/.claude",
        codexConfigDir: "/home/user/.codex",
        geminiConfigDir: "/home/user/.gemini",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept empty string for directory paths", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "",
        codexConfigDir: "",
        geminiConfigDir: "",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept null for directory paths", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: null,
        codexConfigDir: null,
        geminiConfigDir: null,
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept provider IDs", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        currentProviderClaude: "provider-123",
        currentProviderCodex: "provider-456",
        currentProviderGemini: "provider-789",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept skill sync methods", () => {
      for (const method of ["auto", "symlink", "copy"]) {
        const data: SettingsFormData = {
          showInTray: true,
          minimizeToTrayOnClose: false,
          skillSyncMethod: method as "auto" | "symlink" | "copy",
        };

        const result = settingsSchema.safeParse(data);
        expect(result.success).toBe(true);
      }
    });

    it("should accept WebDAV sync configuration", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        webdavSync: {
          enabled: true,
          autoSync: true,
          baseUrl: "https://webdav.example.com",
          username: "user@example.com",
          password: "secure-password",
          remoteRoot: "/remote/path",
          profile: "profile-name",
          status: {
            lastSyncAt: Date.now(),
            lastError: null,
            lastErrorSource: null,
            lastRemoteEtag: "etag-123",
            lastLocalManifestHash: "hash-abc",
            lastRemoteManifestHash: "hash-def",
          },
        },
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });

  describe("edge cases", () => {
    it("should accept directories with spaces", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "/path with spaces/to/config",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept directories with special characters", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "/path/with-dashes_underscores.dots/config",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept directories with unicode characters", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "/路径/到/配置/ディレクトリ",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept Windows paths", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "C:\\Users\\username\\.claude",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should trim whitespace from directory paths", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        claudeConfigDir: "   /path/to/config   ",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.claudeConfigDir).toBe("/path/to/config");
      }
    });

    it("should reject invalid language codes", () => {
      const data = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        language: "fr",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject invalid skill sync method", () => {
      const data = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        skillSyncMethod: "invalid-method",
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should accept partial WebDAV configuration", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        webdavSync: {
          enabled: true,
        },
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept empty WebDAV status", () => {
      const data: SettingsFormData = {
        showInTray: true,
        minimizeToTrayOnClose: false,
        webdavSync: {
          status: {
            lastSyncAt: null,
            lastError: null,
          },
        },
      };

      const result = settingsSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });
});

describe("mcpServerSchema", () => {
  describe("valid data", () => {
    it("should accept valid stdio MCP server", () => {
      const data: McpServerFormData = {
        id: "mcp-fetch",
        name: "Fetch Server",
        server: {
          type: "stdio",
          command: "npx",
          args: ["@modelcontextprotocol/server-fetch"],
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept valid HTTP MCP server", () => {
      const data: McpServerFormData = {
        id: "mcp-http",
        server: {
          type: "http",
          url: "http://localhost:8080",
          headers: { Authorization: "Bearer token" },
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept valid SSE MCP server", () => {
      const data: McpServerFormData = {
        id: "mcp-sse",
        server: {
          type: "sse",
          url: "https://sse.example.com/mcp",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept MCP server with environment variables", () => {
      const data: McpServerFormData = {
        id: "mcp-with-env",
        server: {
          type: "stdio",
          command: "/usr/bin/python",
          args: ["server.py"],
          env: { PYTHON_PATH: "/usr/bin/python", DEBUG: "1" },
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept MCP server with cwd", () => {
      const data: McpServerFormData = {
        id: "mcp-with-cwd",
        server: {
          type: "stdio",
          command: "npm",
          args: ["start"],
          cwd: "/path/to/project",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept MCP server with all optional fields", () => {
      const data: McpServerFormData = {
        id: "complete-mcp",
        name: "Complete Server",
        description: "A complete MCP server configuration",
        tags: ["fetch", "http", "utility"],
        homepage: "https://example.com",
        docs: "https://example.com/docs",
        enabled: true,
        server: {
          type: "http",
          url: "https://api.example.com",
          headers: { "X-API-Key": "secret" },
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept default type as stdio", () => {
      const data: McpServerFormData = {
        id: "default-stdio",
        server: {
          command: "python",
          args: ["server.py"],
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });
  });

  describe("invalid data", () => {
    it("should reject MCP server with missing id", () => {
      const data = {
        server: {
          type: "stdio" as const,
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].path).toContain("id");
      }
    });

    it("should reject empty id", () => {
      const data: McpServerFormData = {
        id: "",
        server: {
          type: "stdio",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject stdio without command", () => {
      const data: McpServerFormData = {
        id: "stdio-no-cmd",
        server: {
          type: "stdio",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].message).toContain("command");
      }
    });

    it("should reject stdio with empty command", () => {
      const data: McpServerFormData = {
        id: "stdio-empty-cmd",
        server: {
          type: "stdio",
          command: "   ",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject HTTP without URL", () => {
      const data: McpServerFormData = {
        id: "http-no-url",
        server: {
          type: "http",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].message).toContain("url");
      }
    });

    it("should reject HTTP with empty URL", () => {
      const data: McpServerFormData = {
        id: "http-empty-url",
        server: {
          type: "http",
          url: "   ",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject HTTP with invalid URL", () => {
      const data: McpServerFormData = {
        id: "http-invalid-url",
        server: {
          type: "http",
          url: "not-a-url",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject SSE without URL", () => {
      const data: McpServerFormData = {
        id: "sse-no-url",
        server: {
          type: "sse",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject invalid type", () => {
      const data = {
        id: "invalid-type",
        server: {
          type: "invalid",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject invalid homepage URL", () => {
      const data: McpServerFormData = {
        id: "bad-homepage",
        homepage: "not-a-url",
        server: {
          type: "stdio",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });

    it("should reject invalid docs URL", () => {
      const data: McpServerFormData = {
        id: "bad-docs",
        docs: "invalid-url",
        server: {
          type: "stdio",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(false);
    });
  });

  describe("edge cases", () => {
    it("should accept command with spaces", () => {
      const data: McpServerFormData = {
        id: "cmd-spaces",
        server: {
          type: "stdio",
          command: "/usr/bin/my python",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept args with special characters", () => {
      const data: McpServerFormData = {
        id: "args-special",
        server: {
          type: "stdio",
          command: "npm",
          args: ["--config=/path/with spaces", "--flag=value&special=chars"],
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept environment variables with complex values", () => {
      const data: McpServerFormData = {
        id: "env-complex",
        server: {
          type: "stdio",
          command: "python",
          env: {
            PATH: "/usr/bin:/usr/local/bin",
            JSON_CONFIG: JSON.stringify({ key: "value" }),
            SPECIAL: "!@#$%^&*()_+-=[]{}|;:',.<>?/`~",
          },
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept HTTP URL with authentication", () => {
      const data: McpServerFormData = {
        id: "http-auth",
        server: {
          type: "http",
          url: "https://user:password@api.example.com:8080/path?query=value",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept unicode characters in id and name", () => {
      const data: McpServerFormData = {
        id: "服务器-🚀-サーバー",
        name: "中文名称 🎯 日本語名前",
        server: {
          type: "stdio",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept tags with unicode", () => {
      const data: McpServerFormData = {
        id: "unicode-tags",
        tags: ["标签", "タグ", "🏷️", "fetch", "api"],
        server: {
          type: "stdio",
          command: "npm",
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept headers with various content types", () => {
      const data: McpServerFormData = {
        id: "headers",
        server: {
          type: "http",
          url: "https://api.example.com",
          headers: {
            "Content-Type": "application/json",
            Accept: "application/json",
            Authorization: "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "X-Custom-Header": "custom-value",
          },
        },
      };

      const result = mcpServerSchema.safeParse(data);
      expect(result.success).toBe(true);
    });

    it("should accept URL with localhost and various ports", () => {
      for (const url of [
        "http://localhost:3000",
        "http://127.0.0.1:8080",
        "https://localhost:443",
      ]) {
        const data: McpServerFormData = {
          id: "localhost",
          server: {
            type: "http",
            url,
          },
        };

        const result = mcpServerSchema.safeParse(data);
        expect(result.success).toBe(true);
      }
    });
  });
});

describe("jsonConfigSchema", () => {
  describe("valid data", () => {
    it("should accept valid JSON object", () => {
      const result = jsonConfigSchema.safeParse('{"key": "value"}');
      expect(result.success).toBe(true);
    });

    it("should accept nested JSON object", () => {
      const result = jsonConfigSchema.safeParse(
        '{"nested": {"deep": {"value": 123}}}',
      );
      expect(result.success).toBe(true);
    });

    it("should accept JSON with various types", () => {
      const result = jsonConfigSchema.safeParse(
        '{"str": "text", "num": 42, "bool": true, "null": null, "arr": [1, 2, 3]}',
      );
      expect(result.success).toBe(true);
    });
  });

  describe("invalid data", () => {
    it("should reject empty string", () => {
      const result = jsonConfigSchema.safeParse("");
      expect(result.success).toBe(false);
    });

    it("should reject JSON array", () => {
      const result = jsonConfigSchema.safeParse("[1, 2, 3]");
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].message).toContain("对象");
      }
    });

    it("should reject null JSON", () => {
      const result = jsonConfigSchema.safeParse("null");
      expect(result.success).toBe(false);
    });

    it("should reject plain string JSON", () => {
      const result = jsonConfigSchema.safeParse('"just a string"');
      expect(result.success).toBe(false);
    });

    it("should reject plain number JSON", () => {
      const result = jsonConfigSchema.safeParse("42");
      expect(result.success).toBe(false);
    });

    it("should reject invalid JSON", () => {
      const result = jsonConfigSchema.safeParse("{invalid}");
      expect(result.success).toBe(false);
    });
  });
});

describe("tomlConfigSchema", () => {
  describe("valid data", () => {
    it("should accept empty string", () => {
      const result = tomlConfigSchema.safeParse("");
      expect(result.success).toBe(true);
    });

    it("should accept whitespace only", () => {
      const result = tomlConfigSchema.safeParse("   \n\n   ");
      expect(result.success).toBe(true);
    });
  });

  describe("invalid TOML syntax", () => {
    it("should reject invalid TOML", () => {
      const result = tomlConfigSchema.safeParse("invalid = [ unclosed");
      expect(result.success).toBe(false);
      if (!result.success) {
        expect(result.error.issues[0].message).toContain("TOML");
      }
    });

    it("should reject TOML with unmatched brackets", () => {
      const result = tomlConfigSchema.safeParse("[section");
      expect(result.success).toBe(false);
    });
  });
});

describe("Type Safety", () => {
  it("should infer ProviderFormData type correctly", () => {
    const data: ProviderFormData = {
      name: "Test",
      settingsConfig: "{}",
    };

    expect(data.name).toBe("Test");
    expect(data.settingsConfig).toBe("{}");
  });

  it("should infer SettingsFormData type correctly", () => {
    const data: SettingsFormData = {
      showInTray: true,
      minimizeToTrayOnClose: false,
    };

    expect(data.showInTray).toBe(true);
  });

  it("should infer McpServerFormData type correctly", () => {
    const data: McpServerFormData = {
      id: "test",
      server: {
        type: "stdio",
        command: "npm",
      },
    };

    expect(data.id).toBe("test");
  });
});
