import { useState, useCallback, useMemo } from "react";
import {
  buildOmoSlimProfilePreview,
  buildOmoProfilePreview,
} from "@/types/omo";

interface UseOmoDraftStateParams {
  initialOmoSettings: Record<string, unknown> | undefined;
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
  mergedOmoJsonPreview: string;
  resetOmoDraftState: () => void;
}

export function useOmoDraftState({
  initialOmoSettings,
  category,
}: UseOmoDraftStateParams): OmoDraftState {
  const isSlim = category === "omo-slim";

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

  const mergedOmoJsonPreview = useMemo(() => {
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
  }, [omoAgents, omoCategories, omoOtherFieldsStr, isSlim]);

  const resetOmoDraftState = useCallback(() => {
    setOmoAgents({});
    setOmoCategories({});
    setOmoOtherFieldsStr("");
  }, []);

  return {
    omoAgents,
    setOmoAgents,
    omoCategories,
    setOmoCategories,
    omoOtherFieldsStr,
    setOmoOtherFieldsStr,
    mergedOmoJsonPreview,
    resetOmoDraftState,
  };
}
