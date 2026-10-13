import type { TFunction } from "i18next";
import type { SkillRepoFailure } from "@/lib/api/skills";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";

export const repoFailureKey = (failure: {
  sourceType?: string;
  sourceHost?: string;
  owner: string;
  name: string;
}) =>
  `${failure.sourceType || "github"}/${failure.sourceHost || "github.com"}/${failure.owner}/${failure.name}`.toLowerCase();

/** 「owner/name（原因）、owner2/name2（原因）」：放进「N 个仓库没有读到：…」横幅 */
export function describeRepoFailures(
  failures: readonly SkillRepoFailure[],
  t: TFunction,
): string {
  return failures
    .map((failure) =>
      t("skillsPage.repoFail.item", {
        repo: `${failure.sourceHost}/${failure.owner}/${failure.name}`,
        reason: skillErrorReason(failure.error, t),
      }),
    )
    .join(t("mcpPage.listSeparator"));
}
