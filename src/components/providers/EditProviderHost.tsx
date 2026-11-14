import { lazy, useCallback, useEffect } from "react";
import type { AppId } from "@/lib/api";
import type { ProviderEditorSave } from "@/lib/api/providers";
import type { Provider } from "@/types";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { preloadOnIdle } from "@/lib/preloadOnIdle";
import {
  useEditProviderRequest,
  type EditProviderStore,
} from "@/components/providers/editProviderStore";

const loadEditProviderDialog = () =>
  import("@/components/providers/EditProviderDialog");
const EditProviderDialog = lazy(() =>
  loadEditProviderDialog().then((m) => ({
    default: m.EditProviderDialog,
  })),
);

interface EditProviderHostProps {
  store: EditProviderStore;
  appId: AppId;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
    editorSave?: ProviderEditorSave;
  }) => Promise<void> | void;
  isProxyTakeover: boolean;
  currentProviderId: string;
}

/**
 * 编辑页的宿主：只有它订阅「正在编辑哪个供应商」，打开、关闭编辑页时 App 和列表不重渲染。
 * 第一次打开后一直挂着（关闭时 `open=false`），和原来 App 里的写法一致。
 */
export function EditProviderHost({
  store,
  appId,
  onSubmit,
  isProxyTakeover,
  currentProviderId,
}: EditProviderHostProps) {
  const request = useEditProviderRequest(store);
  // 编辑页连同表单一百多个模块，启动后空闲时先拉好，第一次点「编辑」不用现等
  useEffect(() => preloadOnIdle(loadEditProviderDialog), []);
  // 关闭后保留最后一次的内容，供关闭动画期间显示
  const shown = useLastValidValue(request);
  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open) store.close();
    },
    [store],
  );

  if (!shown) return null;

  return (
    <EditProviderDialog
      open={request !== null}
      provider={shown.provider}
      onOpenChange={handleOpenChange}
      onSubmit={onSubmit}
      appId={appId}
      isProxyTakeover={isProxyTakeover}
      isCurrent={shown.provider.id === currentProviderId}
      modeView={shown.modeView}
    />
  );
}
