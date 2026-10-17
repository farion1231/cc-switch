import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { REMOTE_COMMANDS, remoteApi } from "@/lib/api/remote";

const invokeMock = vi.fn(async (..._args: unknown[]) => undefined);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const REPO_ROOT = process.cwd();

/**
 * 从 Rust 源码里把 `REMOTE_COMMANDS` 名单读出来 —— 那份名单是
 * `generate_handler!` 门禁（`generate_handler_registers_every_remote_command`）
 * 的另一侧。前端必须与它对齐，两边漂移时这里先红。
 */
function backendRemoteCommands(): string[] {
  const source = readFileSync(
    join(REPO_ROOT, "src-tauri/src/commands/remote.rs"),
    "utf-8",
  );
  const start = source.indexOf("REMOTE_COMMANDS: [&str; 13] = [");
  expect(start, "remote.rs must still declare REMOTE_COMMANDS").toBeGreaterThan(
    -1,
  );
  const body = source.slice(start, source.indexOf("];", start));
  return [...body.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
}

/**
 * 从 Rust 源码读一条 `#[command]` 的形参名，丢掉 Tauri 注入的 `AppHandle`，
 * 折成前端该传的 camelCase payload 键。
 *
 * 为什么逐条对着 Rust 签名核：名字对齐只挡住了「命令没登记」，挡不住**参数**漂移。
 * `approve_pairing` 的 `confirmed_unverified_source` 就是活样本 —— 它是必填形参，
 * 前端少传一个键，这次 invoke 因缺参失败，而报错长相与「命令没登记」一模一样，
 * 排查时会把方向带偏。名单门禁（上面那条）对此完全无感。
 */
function backendPayloadKeys(command: string): string[] {
  const source = readFileSync(
    join(REPO_ROOT, "src-tauri/src/commands/remote.rs"),
    "utf-8",
  );
  // 不带 `(` 一起匹配：有几条命令是 `pub async fn set_remote_config<R: Runtime>(`，
  // 泛型参数夹在名字和形参表中间。
  const decl = new RegExp(`\\bpub (?:async )?fn ${command}\\b`).exec(source);
  expect(decl, `remote.rs must still declare ${command}`).not.toBeNull();

  const open = source.indexOf("(", decl!.index);
  // 数括号而不是找第一个 `)`：`AppHandle<R>` 里没有括号，但 `Result<Vec<ClientMeta>, String>`
  // 这类返回类型在形参表之后 —— 只有配平的深度扫描能稳定切出形参表本身。
  let depth = 0;
  let close = -1;
  for (let i = open; i < source.length; i += 1) {
    if (source[i] === "(") depth += 1;
    else if (source[i] === ")") {
      depth -= 1;
      if (depth === 0) {
        close = i;
        break;
      }
    }
  }
  expect(close, `cannot find the parameter list of ${command}`).toBeGreaterThan(
    open,
  );

  const params = source.slice(open + 1, close);

  return params
    .split(",")
    .map((param) => param.trim())
    .filter(Boolean)
    .map((param) => param.split(":")[0].trim())
    .filter((name) => name !== "app")
    .map((name) =>
      name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase()),
    );
}

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.tsx?$/.test(entry)) out.push(full);
  }
  return out;
}

describe("remoteApi", () => {
  beforeEach(() => {
    invokeMock.mockClear();
  });

  it("covers exactly the thirteen commands the backend registers", () => {
    const backend = backendRemoteCommands();
    expect(backend).toHaveLength(13);
    // 集合相等，不是「至少 13 条」：多一条前端调不动的命令同样是漂移。
    expect([...REMOTE_COMMANDS].sort()).toEqual([...backend].sort());
  });

  it("invokes each lifecycle command with the camelCase payload tauri expects", async () => {
    await remoteApi.startRemoteServer(4000, true);
    expect(invokeMock).toHaveBeenCalledWith("start_remote_server", {
      port: 4000,
      tailscaleEnabled: true,
    });

    await remoteApi.stopRemoteServer();
    expect(invokeMock).toHaveBeenCalledWith("stop_remote_server");

    await remoteApi.restartRemoteServer(8080, false);
    expect(invokeMock).toHaveBeenCalledWith("restart_remote_server", {
      port: 8080,
      tailscaleEnabled: false,
    });

    expect(await remoteApi.checkTailscaleAvailable()).toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("check_tailscale_available");

    await remoteApi.getTailscaleIp();
    expect(invokeMock).toHaveBeenCalledWith("get_tailscale_ip");
  });

  it("invokes the pairing commands with the args the dialog binds", async () => {
    await remoteApi.approvePairing("req-1", "ABCD-EFGH", true, true);
    expect(invokeMock).toHaveBeenCalledWith("approve_pairing", {
      requestId: "req-1",
      userCode: "ABCD-EFGH",
      // 后端 `approve_pairing` 的第三、四个形参都是必填的：漏掉任一个这次 invoke
      // 直接因缺参失败，而失败长相与「命令没登记」一样，排查时会把方向带偏。
      confirmedUnverifiedSource: true,
      confirmedForeignAccount: true,
    });

    // 如实透传，不替用户宣称一次他从没做过的确认。
    invokeMock.mockClear();
    await remoteApi.approvePairing("req-2", "IJKL-MNOP", false, false);
    expect(invokeMock).toHaveBeenCalledWith("approve_pairing", {
      requestId: "req-2",
      userCode: "IJKL-MNOP",
      confirmedUnverifiedSource: false,
      confirmedForeignAccount: false,
    });

    await remoteApi.rejectPairing("req-1");
    expect(invokeMock).toHaveBeenCalledWith("reject_pairing", {
      requestId: "req-1",
    });

    await remoteApi.listPairedClients();
    expect(invokeMock).toHaveBeenCalledWith("list_paired_clients");

    await remoteApi.revokeClient("laptop");
    expect(invokeMock).toHaveBeenCalledWith("revoke_client", {
      clientName: "laptop",
    });
  });

  it("invokes the exposure-boundary commands", async () => {
    await remoteApi.getRemoteConfig();
    expect(invokeMock).toHaveBeenCalledWith("get_remote_config");

    await remoteApi.setRemoteConfig({
      enabled: true,
      port: 4000,
      tailscaleEnabled: false,
      bindScope: "loopback",
      corsMode: "explicitWhitelist",
      originWhitelist: ["https://cli.example.invalid"],
    });
    expect(invokeMock).toHaveBeenCalledWith("set_remote_config", {
      config: expect.objectContaining({ bindScope: "loopback" }),
    });

    await remoteApi.requestBindScopeChange("tailscale");
    expect(invokeMock).toHaveBeenCalledWith("request_bind_scope_change", {
      scope: "tailscale",
    });
  });

  it("sends exactly the payload keys each Rust command declares", async () => {
    // 每条命令的调用点：payload 的期望键由**后端签名**现推，不是在这里再抄一遍。
    const callers: Array<[string, () => Promise<unknown>]> = [
      ["start_remote_server", () => remoteApi.startRemoteServer(4000, true)],
      ["stop_remote_server", () => remoteApi.stopRemoteServer()],
      [
        "restart_remote_server",
        () => remoteApi.restartRemoteServer(4000, true),
      ],
      ["check_tailscale_available", () => remoteApi.checkTailscaleAvailable()],
      ["get_tailscale_ip", () => remoteApi.getTailscaleIp()],
      ["list_network_addresses", () => remoteApi.listNetworkAddresses()],
      [
        "approve_pairing",
        () => remoteApi.approvePairing("req-1", "ABCD-EFGH", true, true),
      ],
      ["reject_pairing", () => remoteApi.rejectPairing("req-1")],
      ["list_paired_clients", () => remoteApi.listPairedClients()],
      ["revoke_client", () => remoteApi.revokeClient("laptop")],
      ["get_remote_config", () => remoteApi.getRemoteConfig()],
      [
        "set_remote_config",
        () =>
          remoteApi.setRemoteConfig({
            enabled: true,
            port: 4000,
            tailscaleEnabled: false,
            bindScope: "loopback",
            corsMode: "explicitWhitelist",
            originWhitelist: [],
          }),
      ],
      [
        "request_bind_scope_change",
        () => remoteApi.requestBindScopeChange("tailscale"),
      ],
    ];

    // 13 条命令必须都被这条门禁走到，加了解释不了的新命令也不许悄悄漏过。
    expect(callers.map(([command]) => command).sort()).toEqual(
      [...REMOTE_COMMANDS].sort(),
    );

    for (const [command, call] of callers) {
      invokeMock.mockClear();
      await call();

      const [target, raw] = invokeMock.mock.calls[0] ?? [];
      expect(target, `${command}: invoke target`).toBe(command);

      const payload = (raw ?? {}) as Record<string, unknown>;
      expect(
        Object.keys(payload).sort(),
        `${command}: payload 的键必须与 Rust 形参逐一对应（少一个键 = 运行时缺参失败）`,
      ).toEqual(backendPayloadKeys(command).sort());
    }
  });

  it("keeps remote command names out of the components", () => {
    const files = walk(join(REPO_ROOT, "src/components")).concat(
      walk(join(REPO_ROOT, "src/hooks")),
    );
    const offenders: string[] = [];

    for (const file of files) {
      const source = readFileSync(file, "utf-8");
      for (const name of REMOTE_COMMANDS) {
        // 只抓字符串字面量里的命令名；`/^[a-z_]+$/` 之外的形式（例如在注释里
        // 提到命令名）不算接线，不该让这条门禁变红。
        if (source.includes(`"${name}"`) || source.includes(`'${name}'`)) {
          offenders.push(`${file.replace(`${REPO_ROOT}/`, "")}: ${name}`);
        }
      }
    }

    expect(
      offenders,
      "组件里不得硬编码远程命令名 —— 统一走 @/lib/api/remote",
    ).toEqual([]);
  });

  it("is re-exported from the api barrel", () => {
    const barrel = readFileSync(
      resolve(REPO_ROOT, "src/lib/api/index.ts"),
      "utf-8",
    );
    expect(barrel).toMatch(/export\s*\{[^}]*\bremoteApi\b[^}]*\}/);
  });
});
