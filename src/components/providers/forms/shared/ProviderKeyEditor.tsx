/**
 * ProviderKeyEditor — single source of truth for the legacy single-key
 * field (`ApiKeySection`) vs the multi-key pool (`ApiKeyListSection`) UI.
 *
 * Decision tree (lives here so the 6 form-field files don't duplicate it):
 * - `providerId` 为 null（新建模式）：
 *     展示单 key 字段 `ApiKeySection`，由表单 `apiKey` 状态直接绑到
 *     `settings_config.apiKey`（写入路径走 `Provider::set_api_key`）。
 * - `providerId` 已存在（编辑模式）：
 *     隐藏单 key 字段——避免与 `ApiKeyListSection` 池管理并存造成
 *     settings_config 不同步（Review #17）。
 *
 * OAuth-managed provider（Copilot / CodexOAuth）由各 form-field 自己在
 * `usesOAuth` 判断中短路掉，本组件不再判断——保证双层保护。
 */

import { ApiKeySection } from "./ApiKeySection";
import { ApiKeyListSection } from "../../ApiKeyListSection";
import type { AppId } from "@/lib/api";
import type { ProviderCategory } from "@/types";

export interface ProviderKeyEditorProps {
  appId: AppId;
  /**
   * `null` while the provider hasn't been saved yet. Drives the single-key
   * vs multi-key UI decision (see file header).
   */
  providerId: string | null;
  /** Show the "API Key" field at all. Some categories (e.g. official Claude) hide it. */
  shouldShowApiKey: boolean;
  /** Show the "Get API Key" / "官方无需 API Key" hint links. */
  shouldShowApiKeyLink?: boolean;
  /** Provider category (used by ApiKeySection for placeholder / partner promo). */
  category?: ProviderCategory;
  /** Provider website URL (used by ApiKeySection's "Get API Key" link). */
  websiteUrl?: string;
  /** Is this a partner provider? Shows the partner promo banner. */
  isPartner?: boolean;
  /** Partner promotion key (e.g. "packycode") for the promo banner. */
  partnerPromotionKey?: string;
  /** Bound to ApiKeySection — only used in new-provider mode. */
  value?: string;
  onChange?: (next: string) => void;
  /**
   * Custom placeholders forwarded to ApiKeySection. Some AppTypes (Codex) need
   * per-app hints; default is ApiKeySection's built-in placeholders.
   */
  placeholder?: ApiKeyPlaceholder;
}

/** Placeholder strings passed through to ApiKeySection. */
type ApiKeyPlaceholder = {
  official?: string;
  thirdParty?: string;
};

export function ProviderKeyEditor({
  appId,
  providerId,
  shouldShowApiKey,
  shouldShowApiKeyLink = false,
  category,
  websiteUrl = "",
  isPartner = false,
  partnerPromotionKey = "",
  value = "",
  onChange,
  placeholder,
}: ProviderKeyEditorProps) {
  if (!shouldShowApiKey) return null;
  // 编辑模式：池管理覆盖单字段
  if (providerId) {
    return <ApiKeyListSection appId={appId} providerId={providerId} />;
  }
  // 新建模式：单 key 字段
  return (
    <ApiKeySection
      value={value}
      onChange={onChange ?? (() => {})}
      category={category}
      shouldShowLink={shouldShowApiKeyLink}
      websiteUrl={websiteUrl}
      isPartner={isPartner}
      partnerPromotionKey={partnerPromotionKey}
      placeholder={{
        official: placeholder?.official ?? "",
        thirdParty: placeholder?.thirdParty ?? "",
      }}
    />
  );
}
