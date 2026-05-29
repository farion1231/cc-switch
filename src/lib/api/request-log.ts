async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

export interface ProxyRequestLogEntry {
  id: string;
  timestamp: string;
  app_type: string;
  provider_name: string;
  provider_id: string;
  method: string;
  endpoint: string;
  model: string;
  is_stream: boolean;
  request_body: unknown;
  response_body: unknown | null;
  status_code: number | null;
  latency_ms: number | null;
  session_id: string | null;
  system_prompt: string | null;
}

export interface RequestLogEventPayload {
  id: string;
  timestamp: string;
  app_type: string;
  provider_name: string;
  method: string;
  endpoint: string;
  model: string;
  is_stream: boolean;
  status_code: number | null;
  latency_ms: number | null;
  has_system_prompt: boolean;
  system_prompt_preview: string | null;
}

export const requestLogApi = {
  async getLogs(): Promise<ProxyRequestLogEntry[]> {
    return await invoke("get_captured_request_logs");
  },

  async getLogDetail(id: string): Promise<ProxyRequestLogEntry | null> {
    return await invoke("get_captured_request_log_detail", { id });
  },

  async clearLogs(): Promise<void> {
    return await invoke("clear_captured_request_logs");
  },

  async setCaptureEnabled(enabled: boolean): Promise<void> {
    return await invoke("set_request_log_capture_enabled", { enabled });
  },

  async isCaptureEnabled(): Promise<boolean> {
    return await invoke("is_request_log_capture_enabled");
  },

  async getMaxEntries(): Promise<number> {
    return await invoke("get_request_log_max_entries");
  },

  async setMaxEntries(max: number): Promise<void> {
    return await invoke("set_request_log_max_entries", { max });
  },
};
