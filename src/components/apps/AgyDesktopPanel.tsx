import { useMemo, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { ProviderList } from "@/components/providers/ProviderList";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import type { Provider } from "@/types";

interface AgyDesktopPanelProps {
  savedProviders?: Record<string, Provider>;
  isLoading?: boolean;
  scrollRef?: RefObject<HTMLDivElement>;
  onCreate?: () => void;
  onDelete: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  searchOpen?: boolean;
  onSearchOpenChange?: (open: boolean) => void;
}

export function AgyDesktopPanel({
  savedProviders,
  isLoading,
  scrollRef,
  onCreate,
  onDelete,
  onOpenWebsite,
  searchOpen,
  onSearchOpenChange,
}: AgyDesktopPanelProps) {
  const { t } = useTranslation();
  const auth = useManagedAuth("agy_oauth", undefined, undefined, {
    agyDesktop: true,
  });
  const requiresReauth = useMemo(() => {
    const usableAccounts = new Set(
      auth.accounts
        .filter((account) => !account.requires_reauth)
        .map(({ id }) => id),
    );
    return new Set(
      Object.keys(savedProviders ?? {}).filter((id) => !usableAccounts.has(id)),
    );
  }, [auth.accounts, savedProviders]);

  return (
    <div
      ref={scrollRef}
      id="main-content"
      className="min-h-0 flex-1 overflow-y-auto scroll-stable overflow-x-hidden px-6 pb-12 pt-4"
    >
      {auth.isStatusError ? (
        <Notice
          tone="danger"
          title={t("authCenter.agy.statusLoadFailed")}
          actions={
            <Button
              variant="neutral"
              size="compact"
              onClick={() => void auth.refetchStatus()}
            >
              {t("authCenter.retry")}
            </Button>
          }
        />
      ) : (
        <ProviderList
          providers={savedProviders ?? {}}
          currentProviderId={auth.defaultAccountId ?? ""}
          appId="agy-desktop"
          isLoading={isLoading || auth.isLoadingStatus}
          onSwitch={(provider) => auth.setDefaultAccount(provider.id)}
          onDelete={onDelete}
          onOpenWebsite={onOpenWebsite}
          onCreate={onCreate}
          searchOpen={searchOpen}
          onSearchOpenChange={onSearchOpenChange}
          agyDesktopAccounts={{
            switching: auth.isSettingDefaultAccount,
            requiresReauth,
          }}
        />
      )}
    </div>
  );
}
