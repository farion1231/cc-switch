import { invoke } from "@tauri-apps/api/core";

export type DiagnosticError =
  | "network"
  | "http"
  | "invalidResponse"
  | "unavailable";

export interface IpReport {
  samples: {
    source: string;
    ip: string | null;
    latencyMs: number;
    error: DiagnosticError | null;
  }[];
  location: {
    ip: string;
    country: string | null;
    region: string | null;
    city: string | null;
    connection: {
      asn: number | null;
      isp: string | null;
      org: string | null;
    } | null;
    timezone: { id: string | null; utc: string | null } | null;
  } | null;
  locationError: DiagnosticError | null;
}

/** An empty URL follows CC Switch's system proxy behavior. */
export function diagnoseIp(url: string): Promise<IpReport> {
  return invoke<IpReport>("diagnose_ip", { url });
}
