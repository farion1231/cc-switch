import { useTranslation } from "react-i18next";
import { Label } from "@/components/ui/label";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";

export function isValidBillingMultiplier(raw: string): boolean {
  const value = raw.trim();
  return (
    value === "" ||
    (/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(value) &&
      Number(value) >= 0 &&
      Number(value) <= 1000)
  );
}

export function BillingMultiplierField({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const invalid = !isValidBillingMultiplier(value);
  return (
    <div className="space-y-2">
      <Label htmlFor="billing-multiplier">
        {t("providerForm.billingMultiplier", {
          defaultValue: "Provider cost multiplier",
        })}
      </Label>
      <ImeSafeInput
        id="billing-multiplier"
        value={value}
        onValueChange={onChange}
        placeholder="1"
        inputMode="decimal"
        aria-invalid={invalid}
        aria-describedby="billing-multiplier-help"
      />
      <p id="billing-multiplier-help" className="text-xs text-fg-2">
        {t("providerForm.billingMultiplierHelp", {
          defaultValue:
            "Apply to all model reference costs for future local proxy requests. 0.5 means half price; blank means 1×. Token counts and past costs stay unchanged. Offline sessions without provider attribution are excluded.",
        })}
      </p>
      {invalid && (
        <p role="alert" className="text-xs text-destructive">
          {t("providerForm.billingMultiplierInvalid", {
            defaultValue:
              "Enter a decimal from 0 to 1000, or leave blank for 1×.",
          })}
        </p>
      )}
    </div>
  );
}
