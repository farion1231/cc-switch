import { invoke } from "@tauri-apps/api/core";

export interface OmpLogin {
  provider: string;
  credentialTypes: ("apiKey" | "oauth" | "unknown")[];
  total: number;
  active: number;
}

export const ompApi = {
  listLogins: () => invoke<OmpLogin[]>("omp_list_logins"),
};
