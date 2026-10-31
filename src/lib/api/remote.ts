import { invoke } from "@tauri-apps/api/core";

/**
 * 远程管理这一族 Tauri 命令的**唯一**前端出口。
 *
 * 为什么单独一个模块：命令名在后端有两份硬约束 —— `commands/remote.rs` 的
 * `#[command]` 定义与 `lib.rs` 的 `generate_handler!` 登记（Rust 侧由
 * `generate_handler_registers_every_remote_command` 门禁）。前端再把名字
 * 抄进各个组件，就成了第三份，抄错一个字母只会得到一个运行时的
 * `command not found`，设置面板上的开关静默失败，没有任何编译期或测试期信号。
 *
 * 收敛到这里之后，`REMOTE_COMMANDS` 让「前端知道的那 13 条」成为可枚举的一份，
 * 与后端名单对齐由 `tests/lib/remoteApi.test.ts` 断言；组件里不再出现命令名字面量
 * （同一条测试扫 `src/components` 与 `src/hooks`）。
 */

/**
 * 前端会 invoke 的远程命令名，与后端 `commands::remote::REMOTE_COMMANDS` 逐名对应。
 *
 * 顺序与后端一致：5 条生命周期 + 1 条地址枚举 + 4 条配对审批 + 3 条暴露边界配置。
 */
export const REMOTE_COMMANDS = [
  "start_remote_server",
  "stop_remote_server",
  "restart_remote_server",
  "check_tailscale_available",
  "get_tailscale_ip",
  "list_network_addresses",
  "approve_pairing",
  "reject_pairing",
  "list_paired_clients",
  "revoke_client",
  "get_remote_config",
  "set_remote_config",
  "request_bind_scope_change",
] as const;

export type RemoteCommandName = (typeof REMOTE_COMMANDS)[number];

/**
 * Mirrors the Rust `BindScope` enum (serde `camelCase`).
 *
 * The declaration order is the contract: `loopback < tailscale < allInterfaces`
 * is exactly the "narrower / wider" order the backend uses to decide whether a
 * change needs the second confirmation step. Widening needs confirmation;
 * narrowing applies immediately.
 */
export type BindScope = "loopback" | "tailscale" | "allInterfaces";

/** Mirrors the Rust `BindScopeChangeResponse` (serde `camelCase`). */
export interface BindScopeChangeResponse {
  /** Whether the requested scope is in effect when this call returns. */
  applied: boolean;
  /** The scope in effect after this call (the old one while a confirmation is pending). */
  bindScope: BindScope;
  /** Only for a pending widening request: the id the confirm endpoint expects. */
  confirmationId?: string;
  /** Only for a pending widening request: Unix seconds. */
  expiresAt?: number;
}

/** Mirrors the Rust `CorsMode` enum (serde `camelCase`). No wildcard variant exists. */
export type CorsMode = "sameOrigin" | "explicitWhitelist";

/** Mirrors the Rust `RemoteConfig` struct (serde `camelCase`). */
export interface RemoteConfig {
  enabled: boolean;
  port: number;
  tailscaleEnabled: boolean;
  bindScope: BindScope;
  corsMode: CorsMode;
  originWhitelist: string[];
}

/** Mirrors the Rust `NetworkAddress` struct (serde `camelCase`). */
export interface NetworkAddress {
  /** OS 给的网卡名，如 `en0` / `utun4`。 */
  interface: string;
  /** 该网卡上的 IPv4 地址（已排除回环与链路本地）。 */
  address: string;
}

/** Mirrors the Rust `ClientMeta` struct — metadata only, never a credential digest. */
export interface PairedClientMeta {
  clientName: string;
  createdAt: number;
  lastUsedAt: number;
}

export const remoteApi = {
  // ========== 生命周期（5 条）==========
  // start / restart 只管端口与 Tailscale；绑定范围 / CORS / 白名单一律由后端从
  // gate 读当前生效值 —— 前端在这两条里没有资格改暴露边界。

  async startRemoteServer(
    port: number,
    tailscaleEnabled: boolean,
  ): Promise<string> {
    return invoke("start_remote_server", { port, tailscaleEnabled });
  },

  async stopRemoteServer(): Promise<string> {
    return invoke("stop_remote_server");
  },

  /** 不广播 SSE 关闭事件，供切换 Tailscale / 改端口复用。 */
  async restartRemoteServer(
    port: number,
    tailscaleEnabled: boolean,
  ): Promise<string> {
    return invoke("restart_remote_server", { port, tailscaleEnabled });
  },

  async checkTailscaleAvailable(): Promise<boolean> {
    return invoke("check_tailscale_available");
  },

  async getTailscaleIp(): Promise<string | null> {
    return invoke("get_tailscale_ip");
  },

  /**
   * 本机所有非回环 IPv4 地址（网卡名 + 地址）。
   *
   * 只用于在「全部网卡」档位下展示暴露面，不参与绑定决策。
   */
  async listNetworkAddresses(): Promise<NetworkAddress[]> {
    return invoke("list_network_addresses");
  },

  // ========== 配对审批与设备管理（4 条）==========
  // 失败一律是同一类错误字符串，后端不区分「不存在 / 过期 / 已用掉」，
  // 前端因此拿不到任何可用来反推配对码存活状态的信息。

  /**
   * 批准一条待审请求。
   *
   * `confirmedUnverifiedSource` **不是可选装饰** —— 后端 `approve_pairing` 的第三个
   * 形参是必填的（`commands/remote.rs`），漏掉它这次 invoke 直接因缺参失败。语义上它
   * 回答的是「用户在界面上勾过『我已知悉来源无法核实』吗」：后端在
   * `Unavailable` / `NotInTailnet` 两类来源下默认不放行（`ApprovalDecision::
   * ConfirmUnavailable`），收不到这道确认就返回 unconfirmed 且**不碰状态机**，请求
   * 仍待审。已核实的来源（Tailnet 节点 / 本机回环）不需要它，必须如实传 `false`
   * —— 传 `true` 等于替用户宣称一次他从没做过的确认。
   *
   * `confirmedForeignAccount` 是**第二道独立的门**：对端属于别的账号（`whois` 的
   * `Node.User.ID` 与 `Self.UserID` 不等）时的默认拒绝由后端状态机自己执行，这个
   * 形参只是把界面上的勾如实传下去。界面上的勾是提示不是防线 —— 但它仍然必须
   * 如实传：不传等于宣称用户没勾过，后端会一直拦到他们补上为止。
   */
  async approvePairing(
    requestId: string,
    userCode: string,
    confirmedUnverifiedSource: boolean,
    confirmedForeignAccount: boolean,
  ): Promise<void> {
    return invoke("approve_pairing", {
      requestId,
      userCode,
      confirmedUnverifiedSource,
      confirmedForeignAccount,
    });
  },

  async rejectPairing(requestId: string): Promise<void> {
    return invoke("reject_pairing", { requestId });
  },

  async listPairedClients(): Promise<PairedClientMeta[]> {
    return invoke("list_paired_clients");
  },

  async revokeClient(clientName: string): Promise<boolean> {
    return invoke("revoke_client", { clientName });
  },

  // ========== 暴露边界配置（3 条）==========
  // 收紧随时可做（setRemoteConfig）；放宽只有一条路：requestBindScopeChange
  // 点火，真正的落盘发生在原生弹窗确认事件回到 Rust 侧之后。

  async getRemoteConfig(): Promise<RemoteConfig> {
    return invoke("get_remote_config");
  },

  /**
   * 写 CORS 模式与 Origin 白名单。
   *
   * `bindScope` 一律被后端忽略 —— 改绑定范围只能走 `requestBindScopeChange`。
   * 参数保留在类型里是为了让调用方不必为「这一项不生效」拆一个额外对象。
   */
  async setRemoteConfig(config: RemoteConfig): Promise<void> {
    return invoke("set_remote_config", { config });
  },

  /**
   * 申请修改绑定范围。
   *
   * - 往窄的档位拨：后端立即落盘并重绑 listener，返回 `applied: true`。
   * - 往宽的档位拨：后端登记一条待确认请求，并由 **Rust 侧弹原生对话框**；
   *   用户点确认之后才生效。渲染层拿不到一个可以自己兑现的 id —— 桌面端
   *   根本没有 `confirm_bind_scope_change` 命令。
   *
   * 浏览器走的是另一组 REST 端点（`/api/remote/config/bind-scope/*`），
   * 因为它的确认方是另一个已认证会话，不是这个 webview。
   */
  async requestBindScopeChange(
    scope: BindScope,
  ): Promise<BindScopeChangeResponse> {
    return invoke("request_bind_scope_change", { scope });
  },
};
