import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Checkbox } from "@/components/ui/checkbox";

type ToggleKey = "hideAttribution" | "alwaysThinking" | "enableTeammates";

interface ClaudeQuickTogglesProps {
  /** Called after a patch is applied to the live file, so the caller can mirror it in the JSON editor. */
  onPatchApplied?: (patch: Record<string, unknown>) => void;
}

const defaultStates: Record<ToggleKey, boolean> = {
  hideAttribution: false,
  alwaysThinking: false,
  enableTeammates: false,
};

function deriveStates(
  cfg: Record<string, unknown>,
): Record<ToggleKey, boolean> {
  const env = cfg?.env as Record<string, unknown> | undefined;
  const attr = cfg?.attribution as Record<string, unknown> | undefined;
  return {
    hideAttribution: attr?.commit === "" && attr?.pr === "",
    alwaysThinking: cfg?.alwaysThinkingEnabled === true,
    enableTeammates: env?.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS === "1",
  };
}

/** Apply RFC 7396 JSON Merge Patch in-place: null = delete, object = recurse, else overwrite. */
function jsonMergePatch(
  target: Record<string, unknown>,
  patch: Record<string, unknown>,
) {
  for (const [key, value] of Object.entries(patch)) {
    if (value === null || value === undefined) {
      delete target[key];
    } else if (typeof value === "object" && !Array.isArray(value)) {
      if (
        typeof target[key] !== "object" ||
        target[key] === null ||
        Array.isArray(target[key])
      ) {
        target[key] = {};
      }
      jsonMergePatch(
        target[key] as Record<string, unknown>,
        value as Record<string, unknown>,
      );
      if (Object.keys(target[key] as Record<string, unknown>).length === 0) {
        delete target[key];
      }
    } else {
      target[key] = value;
    }
  }
}

export { jsonMergePatch };

export function ClaudeQuickToggles({
  onPatchApplied,
}: ClaudeQuickTogglesProps) {
  const { t } = useTranslation();
  const [states, setStates] = useState(defaultStates);

  const readLive = useCallback(async () => {
    try {
      const cfg = await invoke<Record<string, unknown>>(
        "read_live_provider_settings",
        { app: "claude" },
      );
      setStates(deriveStates(cfg));
    } catch {
      // Live file missing or unreadable — show all unchecked
    }
  }, []);

  useEffect(() => {
    readLive();
  }, [readLive]);

  const toggle = useCallback(
    async (key: ToggleKey) => {
      let patch: Record<string, unknown>;
      if (key === "hideAttribution") {
        patch = states.hideAttribution
          ? { attribution: null }
          : { attribution: { commit: "", pr: "" } };
      } else if (key === "alwaysThinking") {
        patch = states.alwaysThinking
          ? { alwaysThinkingEnabled: null }
          : { alwaysThinkingEnabled: true };
      } else {
        patch = states.enableTeammates
          ? { env: { CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS: null } }
          : { env: { CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS: "1" } };
      }

      // Optimistic update
      setStates((prev) => ({ ...prev, [key]: !prev[key] }));

      try {
        await invoke("patch_claude_live_settings", { patch });
        onPatchApplied?.(patch);
      } catch {
        // Revert on failure
        readLive();
      }
    },
    [states, readLive, onPatchApplied],
  );

  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1">
      {(
        [
          ["hideAttribution", "claudeConfig.hideAttribution"],
          ["alwaysThinking", "claudeConfig.alwaysThinking"],
          ["enableTeammates", "claudeConfig.enableTeammates"],
        ] as const
      ).map(([key, i18nKey]) => (
        <label
          key={key}
          className="flex items-center gap-1.5 text-sm cursor-pointer"
        >
          <Checkbox checked={states[key]} onCheckedChange={() => toggle(key)} />
          {t(i18nKey)}
        </label>
      ))}
    </div>
  );
}
