import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type DragEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { copyText, readText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";
import { isTextEditableTarget } from "@/utils/domUtils";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import {
  extractProviderTransferFields,
  prepareProviderTransfer,
  type ProviderTransferFields,
} from "@/utils/providerTransfer";
import {
  PROVIDER_TRANSFER_MIME,
  type ProviderTransferControls,
} from "@/contexts/ProviderTransferContext";

const CLIPBOARD_KIND = "cc-switch-provider-fields";

export function encodeProviderFields(fields: ProviderTransferFields): string {
  return JSON.stringify({ kind: CLIPBOARD_KIND, version: 1, fields });
}

export function decodeProviderFields(
  text: string,
): ProviderTransferFields | null {
  if (typeof text !== "string" || text.length > 2_000_000) return null;
  try {
    const value = JSON.parse(text);
    return value?.kind === CLIPBOARD_KIND &&
      value.version === 1 &&
      value.fields &&
      typeof value.fields === "object" &&
      !Array.isArray(value.fields)
      ? (value.fields as ProviderTransferFields)
      : null;
  } catch {
    return null;
  }
}

export function useProviderTransfer({
  app,
  enabled,
  providers,
  onReady,
}: {
  app: AppId;
  enabled: boolean;
  providers?: Record<string, Provider>;
  onReady: (provider: Provider, warnings: string[], target: AppId) => void;
}) {
  const { t } = useTranslation();
  const [canPaste, setCanPaste] = useState(false);
  const [selectedProvider, setSelectedProvider] = useState<{
    id: string;
    app: AppId;
  } | null>(null);
  const clipboardSequence = useRef(0);
  const navigation = useRef({ app, enabled, revision: 0 });
  if (
    navigation.current.app !== app ||
    navigation.current.enabled !== enabled
  ) {
    navigation.current = {
      app,
      enabled,
      revision: navigation.current.revision + 1,
    };
  }
  const [dragging, setDragging] = useState(false);
  const [sortTarget, setSortTarget] = useState<AppId | null>(null);
  const [busy, setBusy] = useState(false);
  const preparing = useRef(false);
  const selection = useRef<{ provider: Provider; app: AppId } | null>(null);
  const draggedFields = useRef<ProviderTransferFields | null>(null);
  if (providers && selection.current?.app === app) {
    const current = providers[selection.current.provider.id];
    selection.current = current ? { provider: current, app } : null;
  }
  const refreshClipboard = useCallback(async () => {
    const sequence = ++clipboardSequence.current;
    try {
      const value = decodeProviderFields(await readText());
      if (sequence === clipboardSequence.current) setCanPaste(value !== null);
    } catch {
      // An unknown clipboard is still actionable: retry on the explicit paste
      // gesture, or let the platform's native paste event supply the contents.
      if (sequence === clipboardSequence.current) setCanPaste(true);
    }
  }, []);
  useEffect(() => {
    if (!enabled) return;
    void refreshClipboard();
    window.addEventListener("focus", refreshClipboard);
    return () => {
      clipboardSequence.current += 1;
      window.removeEventListener("focus", refreshClipboard);
    };
  }, [enabled, app, refreshClipboard]);
  useTauriEvent("tauri://focus", () => {
    if (enabled) void refreshClipboard();
  });
  const showError = (error: unknown) =>
    toast.error(error instanceof Error ? error.message : String(error));
  const copied = (name: string) =>
    toast.success(
      t("provider.transfer.copied", {
        name,
        defaultValue: "已复制配置，切换应用后可粘贴添加",
      }),
    );

  const pasteFields = async (
    source:
      | ProviderTransferFields
      | (() => Promise<ProviderTransferFields | null>),
    target: AppId,
  ) => {
    if (preparing.current) return;
    preparing.current = true;
    setBusy(true);
    const revision = navigation.current.revision;
    try {
      const value = typeof source === "function" ? await source() : source;
      if (!value || navigation.current.revision !== revision) return;
      const preview = await prepareProviderTransfer(value, target);
      if (navigation.current.revision === revision)
        onReady(preview.provider, preview.warnings, target);
    } catch (error) {
      showError(error);
    } finally {
      preparing.current = false;
      setBusy(false);
    }
  };

  const controls: ProviderTransferControls = {
    copy(provider, sourceApp) {
      try {
        const value = extractProviderTransferFields(provider, sourceApp);
        // Only availability is cached. Paste always reads the system clipboard.
        void copyText(encodeProviderFields(value))
          .then(() => {
            clipboardSequence.current += 1;
            setCanPaste(true);
            copied(provider.name);
          })
          .catch(showError);
      } catch (error) {
        showError(error);
      }
    },
    select(provider, sourceApp) {
      selection.current = { provider, app: sourceApp };
      setSelectedProvider((previous) =>
        previous?.id === provider.id && previous.app === sourceApp
          ? previous
          : { id: provider.id, app: sourceApp },
      );
    },
    startDrag(event: DragEvent, provider, sourceApp) {
      try {
        draggedFields.current = extractProviderTransferFields(
          provider,
          sourceApp,
        );
        event.dataTransfer.effectAllowed = "copy";
        // Only a marker leaves the app: credentials stay in the shared state.
        event.dataTransfer.setData(PROVIDER_TRANSFER_MIME, "cc-switch");
        setDragging(true);
      } catch (error) {
        event.preventDefault();
        showError(error);
      }
    },
    endDrag() {
      draggedFields.current = null;
      setDragging(false);
      setSortTarget(null);
    },
    dragging,
    selectedProvider,
    sortTarget,
    setSortTarget(target, active) {
      setSortTarget(target);
      setDragging(active);
    },
    copyTo(provider, sourceApp, target) {
      try {
        void pasteFields(
          extractProviderTransferFields(provider, sourceApp),
          target,
        );
      } catch (error) {
        showError(error);
      }
    },
    drop(event, target) {
      if (
        !draggedFields.current ||
        !event.dataTransfer.types.includes(PROVIDER_TRANSFER_MIME)
      )
        return;
      event.preventDefault();
      const value = draggedFields.current;
      draggedFields.current = null;
      setDragging(false);
      setSortTarget(null);
      void pasteFields(value, target);
    },
  };

  useEffect(() => {
    if (!enabled) return;
    const copy = (event: ClipboardEvent) => {
      if (
        event.defaultPrevented ||
        isTextEditableTarget(event.target) ||
        window.getSelection()?.toString() ||
        !event.clipboardData ||
        selection.current?.app !== app
      )
        return;
      try {
        const value = extractProviderTransferFields(
          selection.current.provider,
          app,
        );
        event.clipboardData.setData("text/plain", encodeProviderFields(value));
        event.preventDefault();
        clipboardSequence.current += 1;
        setCanPaste(true);
        copied(selection.current.provider.name);
      } catch (error) {
        showError(error);
      }
    };
    const paste = (event: ClipboardEvent) => {
      if (event.defaultPrevented || isTextEditableTarget(event.target)) return;
      const value = decodeProviderFields(
        event.clipboardData?.getData("text/plain") ?? "",
      );
      if (!value) return;
      event.preventDefault();
      clipboardSequence.current += 1;
      setCanPaste(true);
      void pasteFields(value, app);
    };
    window.addEventListener("copy", copy);
    window.addEventListener("paste", paste);
    return () => {
      window.removeEventListener("copy", copy);
      window.removeEventListener("paste", paste);
    };
  });

  return {
    controls,
    busy,
    canPaste,
    paste: () =>
      pasteFields(async () => {
        let text: string;
        try {
          text = await readText();
        } catch {
          throw new Error(t("provider.transfer.clipboardReadFailed"));
        }
        const value = decodeProviderFields(text);
        clipboardSequence.current += 1;
        setCanPaste(value !== null);
        if (!value) toast.info(t("provider.transfer.clipboardEmpty"));
        return value;
      }, app),
  };
}
