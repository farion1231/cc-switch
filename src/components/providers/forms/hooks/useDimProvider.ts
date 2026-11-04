import { useState } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderMeta } from "@/types";
import type { DimModel } from "@/lib/api/dim-oauth";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { useDimOauth } from "./useDimOauth";
import type { DimApiFormat } from "@/lib/dimProviderConfig";

export function useDimProvider({
  appId,
  enabled,
  initialMeta,
  initialModel = "",
  onSelect,
}: {
  appId: AppId;
  enabled: boolean;
  initialMeta?: ProviderMeta;
  initialModel?: string;
  onSelect: (model: DimModel, format: DimApiFormat, issuer: string) => void;
}) {
  const auth = useDimOauth({ enabled });
  const [accountId, setAccountId] = useState<string | null>(() =>
    resolveManagedAccountId(initialMeta, "dim_oauth"),
  );
  const [model, setModel] = useState<string | null>(null);
  const [apiFormat, setApiFormat] = useState<DimApiFormat>(
    initialMeta?.apiFormat ??
      (appId === "gemini" || appId === "agy" ? "gemini_native" : "openai_chat"),
  );
  const [effort, setEffort] = useState(initialMeta?.dimOauthEffort ?? "");
  const [valid, setValid] = useState(false);
  const account = auth.accounts.find(
    (item) => item.id === accountId && !item.requires_reauth,
  );
  const select = (next: DimModel, format: DimApiFormat, nextEffort: string) => {
    setModel(next.id);
    setApiFormat(format);
    setEffort(format === "gemini_native" ? "" : nextEffort);
    if (account)
      onSelect(
        next,
        format,
        account.site === "cn" ? "https://dimagent.cn" : "https://dimagent.com",
      );
  };
  const meta: ProviderMeta = {
    ...initialMeta,
    providerType: "dim_oauth",
    apiFormat,
    dimOauthEffort: effort || undefined,
    authBinding: {
      source: "managed_account",
      authProvider: "dim_oauth",
      accountId: accountId ?? undefined,
    },
    isFullUrl: undefined,
  };
  return {
    enabled,
    account,
    valid: valid && !!account,
    meta,
    fieldsProps: {
      appId,
      accountId,
      model: model ?? initialModel,
      apiFormat,
      effort,
      onValidityChange: setValid,
      onSelect: select,
      onAccountChange: (id: string | null) => {
        setAccountId(id);
        setModel("");
        setEffort("");
        setValid(false);
      },
    },
  };
}
