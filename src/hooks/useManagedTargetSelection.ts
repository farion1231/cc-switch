import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { settingsApi } from "@/lib/api";
import type { ManagedTarget } from "@/types";

export const CODEX_TARGET_STORAGE_KEY = "cc-switch-codex-managed-target";

export function chooseManagedTargetId(
  targets: ManagedTarget[],
  preferredId: string | null,
): string | null {
  const managedCodexTargets = targets.filter(
    (target) => target.app === "codex" && target.managementState === "managed",
  );

  if (
    preferredId &&
    managedCodexTargets.some((target) => target.id === preferredId)
  ) {
    return preferredId;
  }

  return (
    managedCodexTargets.find((target) => target.kind.type === "localWindows")
      ?.id ??
    managedCodexTargets[0]?.id ??
    null
  );
}

export function useManagedTargetSelection(enabled: boolean) {
  const [preferredId, setPreferredId] = useState<string | null>(() =>
    localStorage.getItem(CODEX_TARGET_STORAGE_KEY),
  );
  const query = useQuery({
    queryKey: ["managed-targets"],
    queryFn: () => settingsApi.listManagedTargets(),
    enabled,
  });
  const managedTargets = useMemo(
    () =>
      (query.data ?? []).filter(
        (target) =>
          target.app === "codex" && target.managementState === "managed",
      ),
    [query.data],
  );
  const selectedTargetId = useMemo(
    () => chooseManagedTargetId(managedTargets, preferredId),
    [managedTargets, preferredId],
  );
  const selectedTarget = useMemo(
    () => managedTargets.find((target) => target.id === selectedTargetId),
    [managedTargets, selectedTargetId],
  );

  useEffect(() => {
    if (!enabled || !selectedTargetId || selectedTargetId === preferredId) {
      return;
    }
    localStorage.setItem(CODEX_TARGET_STORAGE_KEY, selectedTargetId);
    setPreferredId(selectedTargetId);
  }, [enabled, preferredId, selectedTargetId]);

  const selectTarget = useCallback((targetId: string) => {
    localStorage.setItem(CODEX_TARGET_STORAGE_KEY, targetId);
    setPreferredId(targetId);
  }, []);

  return {
    ...query,
    managedTargets,
    selectedTarget,
    selectedTargetId,
    selectTarget,
  };
}
