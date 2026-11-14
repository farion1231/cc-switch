import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import { NeutralBadge } from "@/components/mcp/AppMatrix";
import type { UnmanagedSkill } from "@/lib/api/skills";

interface ClaudePluginSkillsSectionProps {
  /** 扫描结果里 `readOnly` 的条目：由 Claude Code 插件提供 */
  skills: UnmanagedSkill[];
}

/**
 * 「由 Claude Code 插件提供」的只读段。
 *
 * 这些 skill 由 Claude Code 的插件系统自行加载，CC Switch 只展示、不管理：
 * 导入一份到 SSOT 再同步回 `~/.claude/skills/`，Claude Code 会因插件 skill 带
 * `plugin:` 命名空间而把两份都加载；插件更新后副本还会过期；Claude Code 也没有
 * 单个插件 skill 的开关。所以这里没有任何操作，只列出名字和来源插件。
 */
export function ClaudePluginSkillsSection({
  skills,
}: ClaudePluginSkillsSectionProps) {
  const { t } = useTranslation();

  const groups = useMemo(() => {
    const byPlugin = new Map<string, UnmanagedSkill[]>();
    for (const skill of skills) {
      const plugin = skill.pluginId ?? "";
      const list = byPlugin.get(plugin);
      if (list) list.push(skill);
      else byPlugin.set(plugin, [skill]);
    }
    return [...byPlugin.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([plugin, list]) => ({
        plugin,
        skills: [...list].sort((a, b) => a.name.localeCompare(b.name)),
      }));
  }, [skills]);

  if (skills.length === 0) return null;

  return (
    <section
      data-testid="claude-plugin-skills"
      aria-labelledby="claude-plugin-skills-title"
      className="mt-4 flex shrink-0 flex-col gap-2"
    >
      <div className="flex items-center gap-1.5">
        <h3
          id="claude-plugin-skills-title"
          className="m-0 text-body font-semibold"
        >
          {t("skillsPage.pluginSkills.title")}
        </h3>
        <NeutralBadge>{skills.length}</NeutralBadge>
        <HelpTip title={t("skillsPage.pluginSkills.helpTitle")}>
          {t("skillsPage.pluginSkills.help")}
        </HelpTip>
      </div>
      <p className="m-0 text-caption text-fg-2">
        {t("skillsPage.pluginSkills.lead")}
      </p>
      <ul className="m-0 flex max-h-[240px] list-none flex-col overflow-y-auto rounded-panel border border-border bg-surface p-0">
        {groups.map((group, index) => (
          <li
            key={group.plugin}
            className={
              index > 0
                ? "flex flex-col gap-1.5 border-t border-border px-3 py-2.5"
                : "flex flex-col gap-1.5 px-3 py-2.5"
            }
          >
            <div className="flex min-w-0 items-center gap-1.5">
              <span className="truncate font-mono text-caption text-fg-2">
                {group.plugin}
              </span>
              <NeutralBadge>{group.skills.length}</NeutralBadge>
            </div>
            <div className="flex flex-wrap gap-1">
              {group.skills.map((skill) => (
                <span
                  key={skill.directory}
                  title={skill.description ?? undefined}
                  className="inline-flex h-[22px] items-center rounded-[6px] bg-subtle px-1.5 text-caption text-fg-1"
                >
                  {skill.name}
                </span>
              ))}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
