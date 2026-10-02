import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

function leafKeys(value: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(value)
    .flatMap(([key, entry]) => {
      const path = prefix ? `${prefix}.${key}` : key;
      return typeof entry === "object" && entry !== null
        ? leafKeys(entry as Record<string, unknown>, path)
        : [path];
    })
    .sort();
}

const cliKeys = [
  "usage",
  "invalidArguments",
  "dataInvalid",
  "hostUnavailable",
  "trustRequired",
  "trustInvalid",
  "targetChanged",
  "passwordRequired",
  "credentialsUnavailable",
  "prepareFailed",
  "sshNotFound",
  "executionFailed",
  "timeout",
  "cancelled",
] as const;

const commandSyntax =
  "cc-switch vps exec --root <VPS-directory> --app <client> --server <UUID> [--timeout <seconds>] -- <remote-command>";

const localeExpectations = [
  {
    language: "en",
    locale: en,
    passwordReuse:
      /bound clients on the same machine.*CC Switch.*local SSH execution/i,
    passwordSafety: /not.*configuration files or Skills.*not.*model/i,
    recovery: /password.*missing.*fingerprint.*changes.*VPS page/i,
    platformScope: /WSL.*containers.*not.*automatically/i,
    roaming: /Windows.*credential roaming.*OS policy/i,
    oneCommand: /exactly one remote shell command string/i,
    noTty: /no TTY/i,
    defaultTimeout: /default.*300 seconds/i,
    stdin: /stdin.*scripts/i,
    fingerprint: /fingerprint.*VPS page/i,
    noPlaintext: /no plaintext fallback/i,
    remoteEffects: /remote command may already have had effects/i,
  },
  {
    language: "zh",
    locale: zh,
    passwordReuse: /已绑定的同机客户端.*CC Switch.*本机 SSH 执行入口/,
    passwordSafety: /不写入配置或 Skill.*不交给模型/,
    recovery: /密码缺失.*指纹变化.*VPS 页面/,
    platformScope: /WSL.*容器.*不自动支持/,
    roaming: /Windows.*凭据漫游.*操作系统策略/,
    oneCommand: /恰好一个远端 shell 命令字符串/,
    noTty: /不分配 TTY/,
    defaultTimeout: /默认.*300 秒/,
    stdin: /stdin.*脚本/,
    fingerprint: /指纹.*VPS 页面/,
    noPlaintext: /不回退到明文/,
    remoteEffects: /远端命令可能已产生效果/,
  },
  {
    language: "zh-TW",
    locale: zhTW,
    passwordReuse: /已綁定的同機用戶端.*CC Switch.*本機 SSH 執行入口/,
    passwordSafety: /不寫入設定或 Skill.*不會交給模型/,
    recovery: /密碼遺失.*指紋變更.*VPS 頁面/,
    platformScope: /WSL.*容器.*不自動支援/,
    roaming: /Windows.*認證資料漫遊.*作業系統政策/,
    oneCommand: /恰好一個遠端 shell 命令字串/,
    noTty: /不配置 TTY/,
    defaultTimeout: /預設.*300 秒/,
    stdin: /stdin.*指令碼/,
    fingerprint: /指紋.*VPS 頁面/,
    noPlaintext: /不會退回明文/,
    remoteEffects: /遠端命令可能已產生效果/,
  },
  {
    language: "ja",
    locale: ja,
    passwordReuse:
      /同じマシン.*関連付けられたクライアント.*CC Switch.*ローカル SSH 実行機能/,
    passwordSafety: /設定ファイルや Skill.*書き込まれず.*モデル.*渡されません/,
    recovery: /パスワードがない.*フィンガープリントが変わった.*VPS ページ/,
    platformScope: /WSL.*コンテナー.*自動.*対応しません/,
    roaming: /Windows.*資格情報のローミング.*OS.*ポリシー/,
    oneCommand: /リモートシェルのコマンド文字列を正確に 1 つ/,
    noTty: /TTY.*割り当てません/,
    defaultTimeout: /既定.*300 秒/,
    stdin: /stdin.*スクリプト/,
    fingerprint: /フィンガープリント.*VPS ページ/,
    noPlaintext: /平文へのフォールバック.*行いません/,
    remoteEffects: /リモートコマンド.*すでに影響.*可能性/,
  },
];

describe("VPS translations", () => {
  it.each(Object.entries({ zh, "zh-TW": zhTW, ja }))(
    "has all VPS keys in %s",
    (_language, locale) => {
      expect(leafKeys(locale.vps)).toEqual(leafKeys(en.vps));
    },
  );

  it.each(localeExpectations)(
    "has all 14 stable CLI messages without runtime interpolation in $language",
    ({ locale }) => {
      const cli = locale.vps.cli;
      expect(cli).toBeDefined();
      expect(Object.keys(cli).sort()).toEqual([...cliKeys].sort());
      for (const message of Object.values(cli)) {
        expect(typeof message).toBe("string");
        expect(message.trim()).not.toBe("");
        expect(message).not.toMatch(/\{\{|\}\}/);
      }
      expect(cli.invalidArguments).toContain("cc-switch vps --help");
      for (const key of [
        "dataInvalid",
        "hostUnavailable",
        "trustRequired",
        "trustInvalid",
        "targetChanged",
        "passwordRequired",
      ] as const) {
        expect(cli[key]).toContain("VPS");
      }
      expect(cli.sshNotFound).toContain("OpenSSH");
    },
  );

  it.each(localeExpectations)(
    "documents the exact non-interactive CLI contract in $language",
    ({ locale, oneCommand, noTty, defaultTimeout, stdin, fingerprint }) => {
      const cli = locale.vps.cli;
      expect(cli).toBeDefined();
      expect(cli.usage).toContain(commandSyntax);
      expect(cli.usage).toMatch(oneCommand);
      expect(cli.usage).toMatch(noTty);
      expect(cli.usage).toMatch(defaultTimeout);
      expect(cli.usage).toMatch(stdin);
      expect(cli.usage).toMatch(fingerprint);
    },
  );

  it.each(localeExpectations)(
    "explains local password reuse and its boundaries in $language",
    ({
      locale,
      passwordReuse,
      passwordSafety,
      recovery,
      platformScope,
      roaming,
    }) => {
      const help = locale.vps.auth.passwordHelp;
      expect(help).toMatch(passwordReuse);
      expect(help).toMatch(passwordSafety);
      expect(help).toMatch(recovery);
      expect(help).toMatch(platformScope);
      expect(help).toMatch(roaming);
    },
  );

  it.each(localeExpectations)(
    "does not promise plaintext fallback or remote rollback in $language",
    ({ locale, noPlaintext, remoteEffects }) => {
      const cli = locale.vps.cli;
      expect(cli).toBeDefined();
      expect(cli.credentialsUnavailable).toMatch(noPlaintext);
      expect(cli.timeout).toMatch(remoteEffects);
      expect(cli.cancelled).toMatch(remoteEffects);
    },
  );
});
