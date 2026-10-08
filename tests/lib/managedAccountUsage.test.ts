import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  findManagedAccountUsers,
  groupUsersByApp,
} from "@/lib/managedAccountUsage";

const provider = (
  id: string,
  extra: Partial<Provider> = {},
  settingsConfig: Record<string, unknown> = {},
): Provider =>
  ({
    id,
    name: id,
    settingsConfig,
    ...extra,
  }) as Provider;

const byId = (...list: Provider[]) =>
  Object.fromEntries(list.map((p) => [p.id, p]));

describe("findManagedAccountUsers", () => {
  const claude = byId(
    // 指定了 a1
    provider("bound-a1", {
      meta: {
        providerType: "github_copilot",
        authBinding: {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId: "a1",
        },
      },
    }),
    // 旧字段 githubAccountId 指定了 a2
    provider("legacy-a2", {
      meta: { providerType: "github_copilot", githubAccountId: "a2" },
    }),
    // 没指定：跟着默认账号
    provider("follows-default", { meta: { providerType: "github_copilot" } }),
    // 没写 providerType 的旧卡按地址认
    provider(
      "by-url",
      {},
      { env: { ANTHROPIC_BASE_URL: "https://api.githubcopilot.com" } },
    ),
    // 别的服务
    provider("xai", { meta: { providerType: "xai_oauth" } }),
    // 普通供应商
    provider("plain", {}, { env: { ANTHROPIC_BASE_URL: "https://x.test" } }),
  );

  it("counts bound providers and default followers for the default account", () => {
    const users = findManagedAccountUsers("github_copilot", ["a1"], "a1", {
      claude,
    });
    expect(users.map((u) => [u.providerId, u.viaDefault])).toEqual([
      ["bound-a1", false],
      ["follows-default", true],
      ["by-url", true],
    ]);
  });

  it("counts only bound providers for a non-default account", () => {
    const users = findManagedAccountUsers("github_copilot", ["a2"], "a1", {
      claude,
    });
    expect(users.map((u) => u.providerId)).toEqual(["legacy-a2"]);
  });

  it("collects every provider when removing all accounts", () => {
    const users = findManagedAccountUsers(
      "github_copilot",
      ["a1", "a2"],
      "a1",
      { claude },
    );
    expect(users.map((u) => u.providerId)).toEqual([
      "bound-a1",
      "legacy-a2",
      "follows-default",
      "by-url",
    ]);
  });

  it("does not count an unbound Codex official card (it uses Codex's own login)", () => {
    const codex = byId(
      provider(
        "codex-official",
        { category: "official", meta: { providerType: "codex_oauth" } },
        { auth: {}, config: "" },
      ),
      provider(
        "codex-managed",
        {
          category: "official",
          meta: {
            providerType: "codex_oauth",
            authBinding: {
              source: "managed_account",
              authProvider: "codex_oauth",
              accountId: "c1",
            },
          },
        },
        { auth: {}, config: "" },
      ),
    );
    const desktop = byId(
      provider("desktop-chatgpt", { meta: { providerType: "codex_oauth" } }),
    );

    const users = findManagedAccountUsers("codex_oauth", ["c1"], "c1", {
      codex,
      "claude-desktop": desktop,
    });
    expect(users.map((u) => [u.appId, u.providerId])).toEqual([
      ["claude-desktop", "desktop-chatgpt"],
      ["codex", "codex-managed"],
    ]);
    expect(groupUsersByApp(users)).toEqual([
      { appId: "claude-desktop", names: ["desktop-chatgpt"] },
      { appId: "codex", names: ["codex-managed"] },
    ]);
  });
});
