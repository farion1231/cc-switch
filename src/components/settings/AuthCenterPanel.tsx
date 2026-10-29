import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";
import { AgyOAuthSection } from "@/components/providers/forms/AgyOAuthSection";
import { ClaudeOfficialSection } from "@/components/providers/forms/ClaudeOfficialSection";
import { KimiAccountLogin } from "@/components/providers/KimiAccountLogin";
import { DshAccountLogin } from "@/components/providers/DshAccount";
import { isMac } from "@/lib/platform";
import type { AuthCenterTarget } from "@/lib/api";

interface AuthCenterPanelProps {
  authScrollTarget?: AuthCenterTarget | null;
  /**
   * 页头没有「?」说明时（供应商表单里打开的全屏「授权中心」）在最上面写一行说明；
   * 侧栏「授权中心」页的页头已经有了，就不再显示。
   */
  showIntro?: boolean;
}

/** 定位到某一组时，卡片上沿离正文顶部留的距离 */
const SECTION_SCROLL_MARGIN = 16;

/** 最近的纵向可滚动祖先（FullScreenPanel 的正文） */
function findScrollContainer(element: HTMLElement): HTMLElement | null {
  for (
    let node = element.parentElement;
    node && node !== document.body;
    node = node.parentElement
  ) {
    const { overflowY } = window.getComputedStyle(node);
    if (overflowY === "auto" || overflowY === "scroll") return node;
  }
  return null;
}

/**
 * 授权中心：Claude 官方账号、GitHub Copilot、ChatGPT、xAI、Kimi Code、DeepSeek Harness，一组一张卡片；
 * macOS 上另有 Antigravity 一组（切 agy 的官方登录，目前只实现了 macOS 钥匙串）。
 * 侧栏的「授权中心」页和供应商表单里「管理账号」打开的全屏页共用。
 */
export function AuthCenterPanel({
  authScrollTarget,
  showIntro = true,
}: AuthCenterPanelProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const claudeOfficialSectionRef = useRef<HTMLDivElement | null>(null);
  const copilotSectionRef = useRef<HTMLDivElement | null>(null);
  const codexOauthSectionRef = useRef<HTMLDivElement | null>(null);
  const xaiOauthSectionRef = useRef<HTMLDivElement | null>(null);
  const agyOauthSectionRef = useRef<HTMLDivElement | null>(null);
  const kimiOauthSectionRef = useRef<HTMLDivElement | null>(null);
  const dshAccountSectionRef = useRef<HTMLDivElement | null>(null);
  const showAgy = isMac();

  useEffect(() => {
    if (!authScrollTarget) return;
    const root = rootRef.current;
    if (!root) return;

    const sectionRef =
      authScrollTarget === "kimi_oauth"
        ? kimiOauthSectionRef
        : authScrollTarget === "dsh_account"
          ? dshAccountSectionRef
          : authScrollTarget === "claude_official"
            ? claudeOfficialSectionRef
            : authScrollTarget === "github_copilot"
              ? copilotSectionRef
              : authScrollTarget === "codex_oauth"
                ? codexOauthSectionRef
                : authScrollTarget === "agy_oauth"
                  ? agyOauthSectionRef
                  : xaiOauthSectionRef;

    const target = sectionRef.current;
    const container = findScrollContainer(root);
    if (!target || !container) return;

    // 不用 scrollIntoView：面板滑入时目标还在右侧视口外，它会连带滚动外壳里
    // overflow-hidden 的 #content-area，把整块内容区横向/纵向带偏。只滚面板自己的正文。
    // 上面几组的账号是异步加载的，加载完会把目标挤下去：内容尺寸变化时重新对齐，
    // 直到用户自己滚动或操作。
    // 对齐期间关掉正文的滚动锚定：否则上方分组变高时浏览器（Chromium / WebView2）
    // 会自己改 scrollTop，被下面当成用户拖了滚动条而提前停止跟随。
    const previousOverflowAnchor = container.style.overflowAnchor;
    container.style.overflowAnchor = "none";
    const inputListeners = new AbortController();
    let resizeObserver: ResizeObserver | undefined;
    let frame = 0;
    let expectedTop: number | null = null;
    const stopPinning = () => {
      cancelAnimationFrame(frame);
      resizeObserver?.disconnect();
      inputListeners.abort();
      container.style.overflowAnchor = previousOverflowAnchor;
    };
    const alignTarget = () => {
      // 位置已被外部改变（比如拖滚动条）：让出控制
      if (expectedTop !== null && container.scrollTop !== expectedTop) {
        stopPinning();
        return;
      }
      const viewportTop =
        container.getBoundingClientRect().top + container.clientTop;
      const delta =
        target.getBoundingClientRect().top -
        viewportTop -
        SECTION_SCROLL_MARGIN;
      container.scrollTop = Math.max(
        0,
        Math.min(
          container.scrollTop + delta,
          container.scrollHeight - container.clientHeight,
        ),
      );
      expectedTop = container.scrollTop;
    };

    const inputOptions = {
      capture: true,
      passive: true,
      signal: inputListeners.signal,
    };
    for (const event of ["wheel", "touchstart", "pointerdown"]) {
      container.addEventListener(event, stopPinning, inputOptions);
    }
    window.addEventListener("keydown", stopPinning, inputOptions);

    frame = requestAnimationFrame(() => {
      alignTarget();
      if (
        inputListeners.signal.aborted ||
        typeof ResizeObserver === "undefined"
      )
        return;
      resizeObserver = new ResizeObserver(() => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(alignTarget);
      });
      resizeObserver.observe(root);
    });

    return stopPinning;
  }, [authScrollTarget]);

  return (
    <div ref={rootRef} className="flex flex-col gap-3">
      {showIntro && (
        <p className="m-0 max-w-[700px] text-caption text-fg-2">
          {t("settings.authCenter.description")}
        </p>
      )}
      <div ref={claudeOfficialSectionRef}>
        <ClaudeOfficialSection helpSide="bottom" />
      </div>
      <div ref={copilotSectionRef}>
        <CopilotAuthSection />
      </div>
      <div ref={codexOauthSectionRef}>
        <CodexOAuthSection showAccountQuota />
      </div>
      <div ref={xaiOauthSectionRef}>
        <XaiOAuthSection mode="manage" helpSide="bottom" />
      </div>
      <div ref={kimiOauthSectionRef}>
        <KimiAccountLogin helpSide="bottom" />
      </div>
      <div ref={dshAccountSectionRef}>
        <DshAccountLogin helpSide={showAgy ? "bottom" : "top"} />
      </div>
      {showAgy && (
        <div ref={agyOauthSectionRef}>
          <AgyOAuthSection helpSide="top" />
        </div>
      )}
    </div>
  );
}
