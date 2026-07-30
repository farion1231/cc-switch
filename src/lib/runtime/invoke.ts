import { invoke } from "@tauri-apps/api/core";
import { getRuntimeSnapshot } from "./store";

export interface RuntimeInvokeOptions {
  remoteCommand?: string;
}

export async function localInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return await invoke<T>(command, args);
}

export async function appInvoke<T>(
  localCommand: string,
  args?: Record<string, unknown>,
  options: RuntimeInvokeOptions = {},
): Promise<T> {
  const runtime = getRuntimeSnapshot();
  if (runtime.status === "local") {
    return await localInvoke<T>(localCommand, args);
  }

  if (runtime.status !== "online") {
    throw new RuntimeInvokeError(
      "REMOTE_OFFLINE",
      runtime.errorMessage || "远程服务器当前不可用",
    );
  }
  if (!options.remoteCommand) {
    throw new RuntimeInvokeError(
      "COMMAND_NOT_EXPOSED",
      `命令尚未支持远程运行: ${localCommand}`,
    );
  }

  // 远端请求必须携带读取业务路由时的同一代快照；后端会在发送前后各校验一次，
  // 从而拒绝目标切换期间到达的旧响应，且绝不能把失败请求降级到本机执行。
  return await localInvoke<T>("remote_invoke", {
    command: options.remoteCommand,
    args: args ?? {},
    generation: runtime.generation,
  });
}

export class RuntimeInvokeError extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "RuntimeInvokeError";
  }
}
