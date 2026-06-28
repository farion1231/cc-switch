import { get, post, put, del } from "../web-client";
import type { AppId } from "./types";
// Re-use the canonical type definitions from the Tauri client so the web and
// desktop skills APIs stay structurally identical and interchangeable.
import type {
  InstalledSkill,
  DiscoverableSkill,
  UnmanagedSkill,
  ImportSkillSelection,
  Skill,
  SkillRepo,
  SkillBackupEntry,
  SkillUninstallResult,
  SkillUpdateInfo,
  MigrationResult,
  SkillsShSearchResult,
} from "../skills";

export type {
  InstalledSkill,
  DiscoverableSkill,
  UnmanagedSkill,
  ImportSkillSelection,
  Skill,
  SkillRepo,
  SkillBackupEntry,
  SkillUninstallResult,
  SkillUpdateInfo,
  MigrationResult,
  SkillsShSearchResult,
};

// Web (HTTP) implementation of the skills API. Mirrors src/lib/api/skills.ts so
// the runtime selector in src/lib/api/index.ts can swap them transparently.
// The backend route (src-tauri/src/web/routes/skills.rs) delegates to the same
// `SkillService` the desktop app uses, so behavior matches exactly.
export const skillsApi = {
  async getInstalled(): Promise<InstalledSkill[]> {
    return get("/skills/installed");
  },

  async getBackups(): Promise<SkillBackupEntry[]> {
    return get("/skills/backups");
  },

  async deleteBackup(backupId: string): Promise<boolean> {
    return del(`/skills/backups/${encodeURIComponent(backupId)}`);
  },

  async installUnified(
    skill: DiscoverableSkill,
    currentApp: AppId,
  ): Promise<InstalledSkill> {
    return post(`/skills/${encodeURIComponent(skill.key)}/install`, {
      skill,
      currentApp,
    });
  },

  async uninstallUnified(id: string): Promise<SkillUninstallResult> {
    return del(`/skills/${encodeURIComponent(id)}/uninstall`);
  },

  async restoreBackup(
    backupId: string,
    currentApp: AppId,
  ): Promise<InstalledSkill> {
    return post(`/skills/backups/${encodeURIComponent(backupId)}/restore`, {
      currentApp,
    });
  },

  async toggleApp(id: string, app: AppId, enabled: boolean): Promise<boolean> {
    return post(`/skills/${encodeURIComponent(id)}/toggle`, { app, enabled });
  },

  async scanUnmanaged(): Promise<UnmanagedSkill[]> {
    return get("/skills/unmanaged");
  },

  async importFromApps(
    imports: ImportSkillSelection[],
  ): Promise<InstalledSkill[]> {
    return post("/skills/import", { imports });
  },

  async discoverAvailable(): Promise<DiscoverableSkill[]> {
    return get("/skills/discover");
  },

  async checkUpdates(): Promise<SkillUpdateInfo[]> {
    return get("/skills/updates");
  },

  async updateSkill(id: string): Promise<InstalledSkill> {
    return put(`/skills/${encodeURIComponent(id)}/update`);
  },

  async migrateStorage(
    target: "cc_switch" | "unified",
  ): Promise<MigrationResult> {
    return post("/skills/migrate-storage", { target });
  },

  async searchSkillsSh(
    query: string,
    limit: number,
    offset: number,
  ): Promise<SkillsShSearchResult> {
    const params = new URLSearchParams({
      query,
      limit: String(limit),
      offset: String(offset),
    });
    return get(`/skills/search?${params.toString()}`);
  },

  async getAll(_app: AppId = "claude"): Promise<Skill[]> {
    return get("/skills");
  },

  async install(_directory: string, _app: AppId = "claude"): Promise<boolean> {
    console.warn(
      "install (legacy) not supported in web mode; use installUnified",
    );
    return false;
  },

  async uninstall(
    _directory: string,
    _app: AppId = "claude",
  ): Promise<SkillUninstallResult> {
    console.warn(
      "uninstall (legacy) not supported in web mode; use uninstallUnified",
    );
    return {};
  },

  async getRepos(): Promise<SkillRepo[]> {
    return get("/skills/repos");
  },

  async addRepo(repo: SkillRepo): Promise<boolean> {
    return post("/skills/repos", repo);
  },

  async removeRepo(owner: string, name: string): Promise<boolean> {
    return del(
      `/skills/repos/${encodeURIComponent(owner)}/${encodeURIComponent(name)}`,
    );
  },

  async openZipFileDialog(): Promise<string | null> {
    // Native file dialogs aren't available in the browser.
    console.warn("open_zip_file_dialog not available in web mode");
    return null;
  },

  async installFromZip(
    _filePath: string,
    _currentApp: AppId,
  ): Promise<InstalledSkill[]> {
    console.warn("install_skills_from_zip not available in web mode");
    return [];
  },
};
