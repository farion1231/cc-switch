import {
  createContext,
  useContext,
  type DragEvent,
  type ReactNode,
} from "react";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { APP_IDS } from "@/config/appConfig";
import { useTranslation } from "react-i18next";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

export const PROVIDER_TRANSFER_MIME = "application/x-cc-switch-provider-fields";

export interface ProviderTransferControls {
  copy: (provider: Provider, app: AppId) => void;
  select: (provider: Provider, app: AppId) => void;
  startDrag: (event: DragEvent, provider: Provider, app: AppId) => void;
  endDrag: () => void;
  dragging: boolean;
  selectedProvider: { id: string; app: AppId } | null;
  sortTarget: AppId | null;
  setSortTarget: (target: AppId | null, dragging: boolean) => void;
  copyTo: (provider: Provider, sourceApp: AppId, targetApp: AppId) => void;
  drop: (event: DragEvent, app: AppId) => void;
}

export function providerTransferDropTarget(
  event: Event,
  delta: { x: number; y: number },
): AppId | null {
  if (!("clientX" in event) || !("clientY" in event)) return null;
  const pointer = event as PointerEvent;
  const element = document.elementFromPoint(
    pointer.clientX + delta.x,
    pointer.clientY + delta.y,
  );
  const app = element
    ?.closest("[data-provider-drop-app]")
    ?.getAttribute("data-provider-drop-app") as AppId | null;
  return app && APP_IDS.includes(app) ? app : null;
}

const ProviderTransferContext = createContext<ProviderTransferControls | null>(
  null,
);

export function ProviderTransferScope({
  value,
  children,
}: {
  value: ProviderTransferControls;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <ProviderTransferContext.Provider value={value}>
      {children}
      <span
        role="status"
        aria-live="polite"
        aria-atomic="true"
        className="sr-only"
      >
        {value.dragging && value.sortTarget
          ? t("provider.transfer.copyTo", {
              app: APP_DISPLAY_NAME[value.sortTarget],
            })
          : ""}
      </span>
    </ProviderTransferContext.Provider>
  );
}

// Cards also appear outside the main application (including isolated previews).
export function useProviderTransferControls() {
  return useContext(ProviderTransferContext);
}
