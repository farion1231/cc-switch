export interface HuaweiCodeAgentAuthStatus {
  authenticated: boolean;
  user?: string;
  expires_at?: number;
  account_id?: string;
}

export interface HuaweiCodeAgentAccount {
  id: string;
  user: string;
  authenticated_at: number;
  is_default: boolean;
  requires_reauth?: boolean;
}

export interface FetchedModel {
  id: string;
  ownedBy: string | null;
}

export interface ExtraAuthHeader {
  name: string;
  value?: string;
  tokenSource?: string;
  valuePrefix?: string;
}
