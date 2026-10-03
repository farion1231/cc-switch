import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import type { VpsAuthMethod, VpsServer } from "@/lib/api/vps";
import { Loader2, PlugZap } from "lucide-react";

export type VpsServerDraft = Omit<VpsServer, "port"> & { port: string };

export function prepareVpsServer(draft: VpsServerDraft): VpsServer | null {
  const port = Number(draft.port);
  const identityFile =
    draft.authMethod === "password"
      ? undefined
      : draft.identityFile?.trim() || undefined;
  const certificateFile =
    draft.authMethod === "certificate"
      ? draft.certificateFile?.trim() || undefined
      : undefined;
  if (
    !draft.name.trim() ||
    !draft.host.trim() ||
    !draft.user.trim() ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535 ||
    ((draft.authMethod === "privateKey" ||
      draft.authMethod === "certificate") &&
      !identityFile) ||
    (draft.authMethod === "certificate" && !certificateFile)
  ) {
    return null;
  }
  return {
    ...draft,
    name: draft.name.trim(),
    purpose: draft.purpose.trim(),
    host: draft.host.trim(),
    user: draft.user.trim(),
    port,
    identityFile,
    certificateFile,
  };
}

interface VpsServerFormProps {
  title: string;
  draft: VpsServerDraft;
  password: string;
  hasSavedPassword: boolean;
  onPasswordChange: (password: string) => void;
  disabled: boolean;
  saving: boolean;
  testing: boolean;
  cancelling: boolean;
  onChange: (draft: VpsServerDraft) => void;
  onSave: () => void;
  onCancel: () => void;
  onTest: () => void;
  onCancelTest: () => void;
}

export function VpsServerForm({
  title,
  draft,
  password,
  hasSavedPassword,
  onPasswordChange,
  disabled,
  saving,
  testing,
  cancelling,
  onChange,
  onSave,
  onCancel,
  onTest,
  onCancelTest,
}: VpsServerFormProps) {
  const { t } = useTranslation();
  const valid =
    prepareVpsServer(draft) !== null &&
    (draft.authMethod !== "password" || password !== "" || hasSavedPassword);
  const field = (
    key: keyof Pick<
      VpsServerDraft,
      | "name"
      | "purpose"
      | "host"
      | "port"
      | "user"
      | "identityFile"
      | "certificateFile"
    >,
  ) => ({
    id: `vps-${key}`,
    value: draft[key] ?? "",
    disabled,
    onChange: (event: React.ChangeEvent<HTMLInputElement>) =>
      onChange({ ...draft, [key]: event.target.value }),
  });

  return (
    <FullScreenPanel
      isOpen
      title={title}
      onClose={onCancel}
      contentClassName="mx-auto max-w-3xl space-y-0"
      footer={
        <div className="flex w-full flex-wrap items-center justify-between gap-2">
          {testing ? (
            <Button
              type="button"
              variant="outline"
              onClick={onCancelTest}
              disabled={cancelling}
            >
              <Loader2
                className="mr-2 h-4 w-4 animate-spin motion-reduce:animate-none"
                aria-hidden="true"
              />
              {t("vps.cancelTest")}
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              onClick={onTest}
              disabled={disabled || !valid}
            >
              <PlugZap className="mr-2 h-4 w-4" aria-hidden="true" />
              {t("vps.testConnection")}
            </Button>
          )}
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={disabled}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              form="vps-server-form"
              disabled={disabled || !valid}
            >
              {saving && (
                <Loader2
                  className="mr-2 h-4 w-4 animate-spin motion-reduce:animate-none"
                  aria-hidden="true"
                />
              )}
              {t("common.save")}
            </Button>
          </div>
        </div>
      }
    >
      <form
        id="vps-server-form"
        aria-label={title}
        className="space-y-6"
        onSubmit={(event) => {
          event.preventDefault();
          if (valid && !disabled) onSave();
        }}
      >
        <fieldset className="min-w-0 space-y-4">
          <legend className="text-sm font-semibold">
            {t("vps.sections.host")}
          </legend>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="vps-name">{t("vps.fields.name")}</Label>
              <Input
                {...field("name")}
                maxLength={128}
                required
                autoFocus
                autoComplete="off"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="vps-purpose">{t("vps.fields.purpose")}</Label>
              <Input
                {...field("purpose")}
                maxLength={2048}
                autoComplete="off"
              />
            </div>
          </div>
          <div className="grid grid-cols-[minmax(0,1fr)_6rem] gap-3">
            <div className="space-y-2">
              <Label htmlFor="vps-host">{t("vps.fields.host")}</Label>
              <Input
                {...field("host")}
                maxLength={253}
                required
                autoComplete="off"
                spellCheck={false}
                placeholder={t("vps.fields.hostPlaceholder")}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="vps-port">{t("vps.fields.port")}</Label>
              <Input
                {...field("port")}
                type="number"
                min={1}
                max={65535}
                step={1}
                required
              />
            </div>
          </div>
        </fieldset>
        <fieldset className="min-w-0 space-y-4">
          <legend className="w-full border-t border-border-default pt-6 text-sm font-semibold">
            {t("vps.sections.authentication")}
          </legend>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="vps-user">{t("vps.fields.user")}</Label>
              <Input
                {...field("user")}
                maxLength={64}
                required
                autoComplete="off"
                spellCheck={false}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="vps-auth-method">{t("vps.auth.method")}</Label>
              <Select
                value={draft.authMethod ?? ""}
                onValueChange={(value) =>
                  onChange({ ...draft, authMethod: value as VpsAuthMethod })
                }
                disabled={disabled}
              >
                <SelectTrigger id="vps-auth-method">
                  <SelectValue placeholder={t("vps.auth.legacy")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="password">
                    {t("vps.auth.password")}
                  </SelectItem>
                  <SelectItem value="privateKey">
                    {t("vps.auth.privateKey")}
                  </SelectItem>
                  <SelectItem value="certificate">
                    {t("vps.auth.certificate")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {draft.authMethod === "password" ? (
            <div className="space-y-2">
              <Label htmlFor="vps-password">{t("vps.auth.password")}</Label>
              <Input
                id="vps-password"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => onPasswordChange(event.target.value)}
                disabled={disabled}
                required={!hasSavedPassword}
                placeholder={t(
                  hasSavedPassword
                    ? "vps.auth.keepPassword"
                    : "vps.auth.passwordPlaceholder",
                )}
                aria-describedby="vps-password-help"
              />
              <p
                id="vps-password-help"
                className="text-xs leading-relaxed text-muted-foreground"
              >
                {t("vps.auth.passwordHelp")}
              </p>
            </div>
          ) : (
            <div className="space-y-2">
              <Label htmlFor="vps-identityFile">
                {t("vps.fields.identityFile")}
              </Label>
              <Input
                {...field("identityFile")}
                required={draft.authMethod !== undefined}
                autoComplete="off"
                spellCheck={false}
                aria-describedby="vps-identity-help"
                placeholder={t("vps.fields.identityPlaceholder")}
              />
              <p
                id="vps-identity-help"
                className="text-xs text-muted-foreground"
              >
                {t("vps.fields.identityHelp")}
              </p>
            </div>
          )}
          {draft.authMethod === "certificate" && (
            <div className="space-y-2">
              <Label htmlFor="vps-certificateFile">
                {t("vps.auth.certificateFile")}
              </Label>
              <Input
                {...field("certificateFile")}
                required
                autoComplete="off"
                spellCheck={false}
                aria-describedby="vps-certificate-help"
                placeholder={t("vps.auth.certificatePlaceholder")}
              />
              <p
                id="vps-certificate-help"
                className="text-xs leading-relaxed text-muted-foreground"
              >
                {t("vps.auth.certificateHelp")}
              </p>
            </div>
          )}
        </fieldset>
        <fieldset className="min-w-0 space-y-3">
          <legend className="w-full border-t border-border-default pt-6 text-sm font-semibold">
            {t("vps.fields.clients")}
          </legend>
          <p className="text-sm leading-relaxed text-muted-foreground">
            {t("vps.formDescription")}
          </p>
          <div className="flex flex-wrap gap-x-6 gap-y-3">
            {SKILLS_APP_IDS.map((app) => (
              <div key={app} className="flex items-center gap-2">
                <Checkbox
                  id={`vps-client-${app}`}
                  checked={Boolean(draft.apps[app])}
                  disabled={disabled}
                  onCheckedChange={(checked) =>
                    onChange({
                      ...draft,
                      apps: { ...draft.apps, [app]: checked === true },
                    })
                  }
                />
                <Label
                  htmlFor={`vps-client-${app}`}
                  className="cursor-pointer select-none font-normal"
                >
                  {t(`apps.${app}`)}
                </Label>
              </div>
            ))}
          </div>
        </fieldset>
      </form>
    </FullScreenPanel>
  );
}
