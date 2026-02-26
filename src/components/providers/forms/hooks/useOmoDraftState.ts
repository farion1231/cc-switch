import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { OmoGlobalConfig } from "@/types/omo";
import {
  mergeOmoConfigPreview,
  mergeOmoSlimConfigPreview,
  buildOmoSlimProfilePreview,
} from "@/types/omo";
import { type OmoGlobalConfigFieldsRef } from "../OmoGlobalConfigFields";
import * as configApi from "@/lib/api/config";
import {
  EMPTY_OMO_GLOBAL_CONFIG,
  buildOmoProfilePreview,
} from "../helpers/opencodeFormUtils";

interface UseOmoDraftStateParams {
  initialOmoSettings: Record<string, unknown> | undefined;
  queriedOmoGlobalConfig: OmoGlobalConfig | undefined;
  isEditMode: boolean;
  appId: string;
  category?: string;
}

export interface OmoDraftState {
  omoAgents: Record<string, Record<string, unknown>>;
  setOmoAgents: React.Dispatch<
    React.SetStateAction<Record<string, Record<string, unknown>>>
  >;
  omoCategories: Record<string, Record<string, unknown>>;
  setOmoCategories: React.Dispatch<
    React.SetStateAction<Record<string, Record<string, unknown>>>
  >;
  omoOtherFieldsStr: string;
  setOmoOtherFieldsStr: React.Dispatch<React.SetStateAction<string>>;
  useOmoCommonConfig: boolean;
  setUseOmoCommonConfig: React.Dispatch<React.SetStateAction<boolean>>;
  isOmoConfigModalOpen: boolean;
  setIsOmoConfigModalOpen: React.Dispatch<React.SetStateAction<boolean>>;
  isOmoSaving: boolean;
  omoGlobalConfigRef: React.RefObject<OmoGlobalConfigFieldsRef | null>;
  omoFieldsKey: number;
  effectiveOmoGlobalConfig: OmoGlobalConfig;
  mergedOmoJsonPreview: string;
  handleOmoGlobalConfigSave: () => Promise<void>;
  handleOmoEditClick: () => void;
  resetOmoDraftState: (useCommonConfig?: boolean) => void;
  setOmoGlobalState: React.Dispatch<
    React.SetStateAction<OmoGlobalConfig | null>
  >;
}

export function useOmoDraftState({
  initialOmoSettings,
  queriedOmoGlobalConfig,
  isEditMode,
  appId,
  category,
}: UseOmoDraftStateParams): OmoDraftState {
  const { t } = useTranslation();
  const isSlim = category === "omo-slim";
  const commonConfigKey = isSlim ? "omo_slim" : "omo";

  const [omoAgents, setOmoAgents] = useState<
    Record<string, Record<string, unknown>>
  >(
    () =>
      (initialOmoSettings?.agents as Record<string, Record<string, unknown>>) ||
      {},
  );
  const [omoCategories, setOmoCategories] = useState<
    Record<string, Record<string, unknown>>
  >(
    () =>
      (initialOmoSettings?.categories as Record<
        string,
        Record<string, unknown>
      >) || {},
  );
  const [omoOtherFieldsStr, setOmoOtherFieldsStr] = useState(() => {
    const otherFields = initialOmoSettings?.otherFields;
    return otherFields ? JSON.stringify(otherFields, null, 2) : "";
  });

  const [omoGlobalState, setOmoGlobalState] = useState<OmoGlobalConfig | null>(
    null,
  );

  const [isOmoConfigModalOpen, setIsOmoConfigModalOpen] = useState(false);
  const [useOmoCommonConfig, setUseOmoCommonConfig] = useState(() => {
    const raw = initialOmoSettings?.useCommonConfig;
    return typeof raw === "boolean" ? raw : true;
  });
  const [isOmoSaving, setIsOmoSaving] = useState(false);
  const omoGlobalConfigRef = useRef<OmoGlobalConfigFieldsRef>(null);
  const [omoFieldsKey, setOmoFieldsKey] = useState(0);
  const effectiveOmoGlobalConfig =
    omoGlobalState ?? queriedOmoGlobalConfig ?? EMPTY_OMO_GLOBAL_CONFIG;

  const mergedOmoJsonPreview = useMemo(() => {
    if (useOmoCommonConfig) {
      if (isSlim) {
        const merged = mergeOmoSlimConfigPreview(
          effectiveOmoGlobalConfig,
          omoAgents,
          omoOtherFieldsStr,
        );
        return JSON.stringify(merged, null, 2);
      }
      const merged = mergeOmoConfigPreview(
        effectiveOmoGlobalConfig,
        omoAgents,
        omoCategories,
        omoOtherFieldsStr,
      );
      return JSON.stringify(merged, null, 2);
    } else {
      if (isSlim) {
        return JSON.stringify(
          buildOmoSlimProfilePreview(omoAgents, omoOtherFieldsStr),
          null,
          2,
        );
      }
      return JSON.stringify(
        buildOmoProfilePreview(omoAgents, omoCategories, omoOtherFieldsStr),
        null,
        2,
      );
    }
  }, [
    useOmoCommonConfig,
    effectiveOmoGlobalConfig,
    omoAgents,
    omoCategories,
    omoOtherFieldsStr,
    isSlim,
  ]);

  // Auto-detect whether common config has content for new OMO/OMO Slim profiles
  useEffect(() => {
    if (
      appId !== "opencode" ||
      (category !== "omo" && category !== "omo-slim") ||
      isEditMode
    )
      return;
    let active = true;
    (async () => {
      let next = false;
      try {
        const raw = await configApi.getCommonConfigSnippet(commonConfigKey);
        if (raw) {
          const parsed = JSON.parse(raw) as Record<string, unknown>;
          next = Object.keys(parsed).some(
            (k) => k !== "id" && k !== "updatedAt",
          );
        }
      } catch {}
      if (active) setUseOmoCommonConfig(next);
    })();
    return () => {
      active = false;
    };
  }, [appId, category, isEditMode, commonConfigKey]);

  const handleOmoGlobalConfigSave = useCallback(async () => {
    if (!omoGlobalConfigRef.current) return;
    setIsOmoSaving(true);
    try {
      const config = omoGlobalConfigRef.current.buildCurrentConfigStrict();
      await configApi.setCommonConfigSnippet(
        commonConfigKey,
        JSON.stringify(config),
      );
      setIsOmoConfigModalOpen(false);
      toast.success(
        t("omo.globalConfigSaved", { defaultValue: "Global config saved" }),
      );
    } catch (err) {
      toast.error(String(err));
    } finally {
      setIsOmoSaving(false);
    }
  }, [t, commonConfigKey]);

  const handleOmoEditClick = useCallback(() => {
    setOmoFieldsKey((k) => k + 1);
    setIsOmoConfigModalOpen(true);
  }, []);

  const resetOmoDraftState = useCallback((useCommonConfig = true) => {
    setOmoAgents({});
    setOmoCategories({});
    setOmoOtherFieldsStr("");
    setUseOmoCommonConfig(useCommonConfig);
  }, []);

  return {
    omoAgents,
    setOmoAgents,
    omoCategories,
    setOmoCategories,
    omoOtherFieldsStr,
    setOmoOtherFieldsStr,
    useOmoCommonConfig,
    setUseOmoCommonConfig,
    isOmoConfigModalOpen,
    setIsOmoConfigModalOpen,
    isOmoSaving,
    omoGlobalConfigRef,
    omoFieldsKey,
    effectiveOmoGlobalConfig,
    mergedOmoJsonPreview,
    handleOmoGlobalConfigSave,
    handleOmoEditClick,
    resetOmoDraftState,
    setOmoGlobalState,
  };
}
