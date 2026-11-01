import { useTranslation } from "react-i18next";
import { ProviderIconBox } from "@/components/ProviderIconBox";
import { HelpTip } from "@/components/ui/help-tip";
import { getIconMetadata } from "@/icons/extracted";
import type { OmpLogin } from "@/lib/api/omp";
import { CardChipBadge } from "./ProviderCard";
import type { CardChip } from "./presentation";

const BUILTIN_PROVIDERS = new Map([
  ["opencode-go", { name: "OpenCode Go", icon: "opencode" }],
  ["openai-codex", { name: "OpenAI Codex", icon: "openai" }],
]);

export function ompLoginPresentation(provider: string) {
  return (
    BUILTIN_PROVIDERS.get(provider) ?? {
      name: getIconMetadata(provider)?.displayName ?? provider,
      icon: provider,
    }
  );
}

export function OmpLoginCard({ login }: { login: OmpLogin }) {
  const { t } = useTranslation();
  const { name, icon } = ompLoginPresentation(login.provider);
  const chips: CardChip[] = [
    { key: "login", label: t("omp.login.chip"), tone: "outline" },
    ...login.credentialTypes.map((type) => ({
      key: type,
      label: t(`omp.login.${type}`),
      tone: "outline" as const,
    })),
  ];
  if (login.active === 0) {
    chips.push({
      key: "disabled",
      label: t("omp.login.disabled"),
      tone: "warning",
    });
  }

  return (
    <div className="group relative rounded-panel border border-border bg-surface transition-[border-color,box-shadow,background-color] duration-150 hover:border-border-strong hover:shadow-v7-sm">
      <div className="flex min-h-[58px] items-center gap-3 py-2.5 pe-3 ps-2">
        <span className="w-4 shrink-0" />
        <ProviderIconBox
          icon={icon}
          name={name}
          className="bg-white text-neutral-900"
          fallbackClassName="bg-transparent text-neutral-600"
        />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-1.5">
            <h3
              className="m-0 min-w-0 truncate text-strong text-fg-1"
              title={name}
            >
              {name}
            </h3>
            {chips.map((chip) => (
              <CardChipBadge key={chip.key} chip={chip} />
            ))}
          </div>
          <div className="mt-0.5 truncate text-caption text-fg-2">
            {login.provider}
          </div>
        </div>
        {login.total > 1 && (
          <span className="shrink-0 text-caption tabular-nums text-fg-2">
            {t("omp.login.accounts", { count: login.total })}
          </span>
        )}
        <HelpTip title={t("omp.login.chip")}>
          {t("omp.login.managedHint")}
        </HelpTip>
      </div>
    </div>
  );
}
