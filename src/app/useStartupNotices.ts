import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api";
import type { EnvConflict } from "@/types/env";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";

const ENV_BANNER_DISMISSED_KEY = "env_banner_dismissed";

/**
 * 启动期提醒：环境变量冲突检测 + 配置/Skills 迁移结果。
 */
export function useStartupNotices(activeApp: AppId) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  // 启动时全量检测环境变量冲突
  useEffect(() => {
    const checkEnvOnStartup = async () => {
      try {
        const allConflicts = await checkAllEnvConflicts();
        const flatConflicts = Object.values(allConflicts).flat();

        if (flatConflicts.length > 0) {
          setEnvConflicts(flatConflicts);
          if (!sessionStorage.getItem(ENV_BANNER_DISMISSED_KEY)) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on startup:",
          error,
        );
      }
    };

    void checkEnvOnStartup();
  }, []);

  // 配置迁移结果
  useEffect(() => {
    const checkMigration = async () => {
      try {
        const migrated = await invoke<boolean>("get_migration_result");
        if (migrated) {
          toast.success(
            t("migration.success", { defaultValue: "配置迁移成功" }),
            { closeButton: true },
          );
        }
      } catch (error) {
        console.error("[App] Failed to check migration result:", error);
      }
    };

    void checkMigration();
  }, [t]);

  // Skills SSOT 迁移结果
  useEffect(() => {
    const checkSkillsMigration = async () => {
      try {
        const result = await invoke<{ count: number; error?: string } | null>(
          "get_skills_migration_result",
        );
        if (result?.error) {
          toast.error(t("migration.skillsFailed"), {
            description: t("migration.skillsFailedDescription"),
            closeButton: true,
          });
          console.error("[App] Skills SSOT migration failed:", result.error);
          return;
        }
        if (result && result.count > 0) {
          toast.success(t("migration.skillsSuccess", { count: result.count }), {
            closeButton: true,
          });
          await queryClient.invalidateQueries({ queryKey: ["skills"] });
        }
      } catch (error) {
        console.error("[App] Failed to check skills migration result:", error);
      }
    };

    void checkSkillsMigration();
  }, [t, queryClient]);

  // 切换应用时增量检测该应用的冲突
  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      try {
        const conflicts = await checkEnvConflicts(activeApp);

        if (conflicts.length > 0) {
          setEnvConflicts((prev) => {
            const existingKeys = new Set(
              prev.map((c) => `${c.varName}:${c.sourcePath}`),
            );
            const newConflicts = conflicts.filter(
              (c) => !existingKeys.has(`${c.varName}:${c.sourcePath}`),
            );
            return [...prev, ...newConflicts];
          });
          if (!sessionStorage.getItem(ENV_BANNER_DISMISSED_KEY)) {
            setShowEnvBanner(true);
          }
        }
      } catch (error) {
        console.error(
          "[App] Failed to check environment conflicts on app switch:",
          error,
        );
      }
    };

    void checkEnvOnSwitch();
  }, [activeApp]);

  const dismissEnvBanner = useCallback(() => {
    setShowEnvBanner(false);
    sessionStorage.setItem(ENV_BANNER_DISMISSED_KEY, "true");
  }, []);

  const recheckEnvConflicts = useCallback(async () => {
    try {
      const allConflicts = await checkAllEnvConflicts();
      const flatConflicts = Object.values(allConflicts).flat();
      setEnvConflicts(flatConflicts);
      if (flatConflicts.length === 0) {
        setShowEnvBanner(false);
      }
    } catch (error) {
      console.error("[App] Failed to re-check conflicts after deletion:", error);
    }
  }, []);

  return {
    envConflicts,
    showEnvBanner,
    dismissEnvBanner,
    recheckEnvConflicts,
  };
}
