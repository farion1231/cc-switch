import { useCallback, useState } from "react";
import type { AppId } from "@/lib/api/types";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { useToggleSkillApp } from "@/hooks/useSkills";

const STORAGE_KEY = "cc-switch:skills:install-to";
const DEFAULT_TARGETS: AppId[] = ["claude", "codex"];

function readStored(): AppId[] | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    const apps = parsed.filter((app): app is AppId =>
      SKILLS_APP_IDS.includes(app as AppId),
    );
    return apps.length ? apps : null;
  } catch {
    return null;
  }
}

function writeStored(apps: AppId[]) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(apps));
  } catch {
    // 只是记住上次的选择，存不了不影响安装
  }
}

/**
 * 「安装到」：发现、ZIP、备份恢复都显式装进这里选的应用（默认记住上次的选择）。
 * 后端 install_skill_unified 只收一个应用，所以先装到第一个、再逐个启用其余的，不是原子的。
 */
export function useSkillInstallTargets(visibleAppIds: readonly AppId[]) {
  const [stored, setStored] = useState<AppId[]>(
    () => readStored() ?? DEFAULT_TARGETS,
  );
  const toggleMutation = useToggleSkillApp();

  // 只算看得见的应用；一个都不剩时退回第一个可见应用
  const targets = SKILLS_APP_IDS.filter(
    (app) => stored.includes(app) && visibleAppIds.includes(app),
  );
  const effective =
    targets.length > 0 ? targets : visibleAppIds.slice(0, 1).map((app) => app);

  const setTargets = useCallback((apps: AppId[]) => {
    setStored(apps);
    writeStored(apps);
  }, []);

  /**
   * 先用 first 应用调用 install，再把其余应用逐个打开。
   * 返回装好的 Skill 和没能启用的应用（不抛错；install 本身失败才抛）。
   */
  const installTo = async <T extends { id: string }>(
    install: (firstApp: AppId) => Promise<T[]>,
    apps: AppId[] = effective,
  ) => {
    const [first, ...rest] = apps;
    const installed = await install(first ?? "claude");
    const failures: Array<{ id: string; app: AppId; error: unknown }> = [];
    for (const skill of installed) {
      for (const app of rest) {
        try {
          await toggleMutation.mutateAsync({
            id: skill.id,
            app,
            enabled: true,
          });
        } catch (error) {
          failures.push({ id: skill.id, app, error });
        }
      }
    }
    return { installed, failures };
  };

  return { targets: effective, setTargets, installTo };
}
