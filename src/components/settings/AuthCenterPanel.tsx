import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";
import type { ManagedAuthProvider } from "@/lib/api";

interface AuthCenterPanelProps {
  authScrollTarget?: ManagedAuthProvider | null;
  /**
   * 页头没有「?」说明时（供应商表单里打开的全屏「授权中心」）在最上面写一行说明；
   * 侧栏「授权中心」页的页头已经有了，就不再显示。
   */
  showIntro?: boolean;
}

/**
 * 授权中心（v7 Auth 画板）：GitHub Copilot、ChatGPT、xAI 三组账号，一组一张卡片。
 * 侧栏的「授权中心」页和供应商表单里「管理账号」打开的全屏页共用。
 */
export function AuthCenterPanel({
  authScrollTarget,
  showIntro = true,
}: AuthCenterPanelProps) {
  const { t } = useTranslation();
  const copilotSectionRef = useRef<HTMLDivElement | null>(null);
  const codexOauthSectionRef = useRef<HTMLDivElement | null>(null);
  const xaiOauthSectionRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!authScrollTarget) return;

    const sectionRef =
      authScrollTarget === "github_copilot"
        ? copilotSectionRef
        : authScrollTarget === "codex_oauth"
          ? codexOauthSectionRef
          : xaiOauthSectionRef;

    const frame = requestAnimationFrame(() => {
      const prefersReducedMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;

      sectionRef.current?.scrollIntoView({
        behavior: prefersReducedMotion ? "auto" : "smooth",
        block: "start",
      });
    });

    return () => cancelAnimationFrame(frame);
  }, [authScrollTarget]);

  return (
    <div className="flex flex-col gap-3">
      {showIntro && (
        <p className="m-0 max-w-[700px] text-caption text-fg-2">
          {t("settings.authCenter.description")}
        </p>
      )}
      <div ref={copilotSectionRef} className="scroll-mt-4">
        <CopilotAuthSection />
      </div>
      <div ref={codexOauthSectionRef} className="scroll-mt-4">
        <CodexOAuthSection showAccountQuota />
      </div>
      <div ref={xaiOauthSectionRef} className="scroll-mt-4">
        <XaiOAuthSection mode="manage" helpSide="top" />
      </div>
    </div>
  );
}
