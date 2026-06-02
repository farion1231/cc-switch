export type ProviderCategory =
  | "official" // 瀹樻柟
  | "cn_official" // 寮€婧愬畼鏂癸紙鍘?鍥戒骇瀹樻柟"锛?  | "cloud_provider" // 浜戞湇鍔″晢锛圓WS Bedrock 绛夛級
  | "aggregator" // 鑱氬悎缃戠珯
  | "third_party" // 绗笁鏂逛緵搴斿晢
  | "custom" // 鑷畾涔?  | "omo" // Oh My OpenCode
  | "omo-slim"; // Oh My OpenCode Slim

export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, any>; // 搴旂敤閰嶇疆瀵硅薄锛欳laude 涓?settings.json锛汣odex 涓?{ auth, config }
  websiteUrl?: string;
  // 鏂板锛氫緵搴斿晢鍒嗙被锛堢敤浜庡樊寮傚寲鎻愮ず/鑳藉姏寮€鍏筹級
  category?: ProviderCategory;
  createdAt?: number; // 娣诲姞鏃堕棿鎴筹紙姣锛?  sortIndex?: number; // 鎺掑簭绱㈠紩锛堢敤浜庤嚜瀹氫箟鎷栨嫿鎺掑簭锛?  // 澶囨敞淇℃伅
  notes?: string;
  // 鏂板锛氭槸鍚︿负鍟嗕笟鍚堜綔浼欎即
  isPartner?: boolean;
  // 鍙€夛細渚涘簲鍟嗗厓鏁版嵁锛堜粎瀛樹簬 ~/.cc-switch/config.json锛屼笉鍐欏叆 live 閰嶇疆锛?  meta?: ProviderMeta;
  // 鍥炬爣閰嶇疆
  icon?: string; // 鍥炬爣鍚嶇О锛堝 "openai", "anthropic"锛?  iconColor?: string; // 鍥炬爣棰滆壊锛圚ex 鏍煎紡锛屽 "#00A67E"锛?  // 鏄惁鍔犲叆鏁呴殰杞Щ闃熷垪
  inFailoverQueue?: boolean;
}

export interface AppConfig {
  providers: Record<string, Provider>;
  current: string;
}

// 鑷畾涔夌鐐归厤缃?export interface CustomEndpoint {
  url: string;
  addedAt: number;
  lastUsed?: number;
}

// 绔偣鍊欓€夐」锛堢敤浜庣鐐规祴閫熷脊绐楋級
export interface EndpointCandidate {
  id?: string;
  url: string;
  isCustom?: boolean;
}

import type { TemplateType } from "./config/constants";

// 鐢ㄩ噺鏌ヨ鑴氭湰閰嶇疆
export interface UsageScript {
  enabled: boolean; // 鏄惁鍚敤鐢ㄩ噺鏌ヨ
  language: "javascript"; // 鑴氭湰璇█
  code: string; // 鑴氭湰浠ｇ爜锛圝SON 鏍煎紡閰嶇疆锛?  timeout?: number; // 瓒呮椂鏃堕棿锛堢锛岄粯璁?10锛?  templateType?: TemplateType; // 妯℃澘绫诲瀷锛堢敤浜庡悗绔垽鏂獙璇佽鍒欙級
  apiKey?: string; // 鐢ㄩ噺鏌ヨ涓撶敤鐨?API Key锛堥€氱敤妯℃澘浣跨敤锛?  baseUrl?: string; // 鐢ㄩ噺鏌ヨ涓撶敤鐨?Base URL锛堥€氱敤鍜?NewAPI 妯℃澘浣跨敤锛?  accessToken?: string; // 璁块棶浠ょ墝锛圢ewAPI 妯℃澘浣跨敤锛?  userId?: string; // 鐢ㄦ埛ID锛圢ewAPI 妯℃澘浣跨敤锛?  codingPlanProvider?: string; // Coding Plan 渚涘簲鍟嗘爣璇嗭紙濡?"kimi", "zhipu", "minimax"锛?  autoQueryInterval?: number; // 鑷姩鏌ヨ闂撮殧锛堝崟浣嶏細鍒嗛挓锛? 琛ㄧず绂佺敤锛?  autoIntervalMinutes?: number; // 鑷姩鏌ヨ闂撮殧锛堝垎閽燂級- 鍒悕瀛楁
  request?: {
    // 璇锋眰閰嶇疆
    url?: string; // 璇锋眰 URL
    method?: string; // HTTP 鏂规硶
    headers?: Record<string, string>; // 璇锋眰澶?    body?: any; // 璇锋眰浣?  };
}

const DEFAULT_USAGE_SCRIPT: UsageScript = {
  enabled: false,
  language: "javascript",
  code: "",
  timeout: 10,
  autoQueryInterval: 5,
};

export function createUsageScript(
  overrides?: Partial<UsageScript>,
): UsageScript {
  return { ...DEFAULT_USAGE_SCRIPT, ...overrides };
}

// 鍗曚釜濂楅鐢ㄩ噺鏁版嵁
export interface UsageData {
  planName?: string; // 濂楅鍚嶇О锛堝彲閫夛級
  extra?: string; // 鎵╁睍瀛楁锛屽彲鑷敱琛ュ厖闇€瑕佸睍绀虹殑鏂囨湰锛堝彲閫夛級
  isValid?: boolean; // 濂楅鏄惁鏈夋晥锛堝彲閫夛級
  invalidMessage?: string; // 澶辨晥鍘熷洜璇存槑锛堝彲閫夛紝褰?isValid 涓?false 鏃舵樉绀猴級
  total?: number; // 鎬婚搴︼紙鍙€夛級
  used?: number; // 宸茬敤棰濆害锛堝彲閫夛級
  remaining?: number; // 鍓╀綑棰濆害锛堝彲閫夛級
  unit?: string; // 鍗曚綅锛堝彲閫夛級
}

// 鐢ㄩ噺鏌ヨ缁撴灉锛堟敮鎸佸濂楅锛?export interface UsageResult {
  success: boolean;
  data?: UsageData[]; // 鏀逛负鏁扮粍锛屾敮鎸佽繑鍥炲涓椁?  error?: string;
}

// 渚涘簲鍟嗗崟鐙殑妯″瀷娴嬭瘯閰嶇疆
export interface ProviderTestConfig {
  // 鏄惁鍚敤鍗曠嫭閰嶇疆锛坒alse 鏃朵娇鐢ㄥ叏灞€閰嶇疆锛?  enabled: boolean;
  // 娴嬭瘯鐢ㄧ殑妯″瀷鍚嶇О锛堣鐩栧叏灞€閰嶇疆锛?  testModel?: string;
  // 瓒呮椂鏃堕棿锛堢锛?  timeoutSecs?: number;
  // 娴嬭瘯鎻愮ず璇?  testPrompt?: string;
  // 闄嶇骇闃堝€硷紙姣锛?  degradedThresholdMs?: number;
  // 鏈€澶ч噸璇曟鏁?  maxRetries?: number;
}

export type AuthBindingSource = "provider_config" | "managed_account";

export interface AuthBinding {
  source: AuthBindingSource;
  authProvider?: string;
  accountId?: string;
}

export interface ClaudeDesktopModelRoute {
  model: string;
  labelOverride?: string;
  supports1m?: boolean;
}

export type CodexChatThinkingParam =
  | "none"
  | "thinking"
  | "enable_thinking"
  | "reasoning_split";

export type CodexChatEffortParam =
  | "none"
  | "reasoning_effort"
  // OpenRouter 鍘熺敓褰掍竴鍖栧璞?reasoning:{effort}锛堝尯鍒簬椤跺眰 OpenAI 鍒悕 reasoning_effort锛?  | "reasoning.effort";

export type CodexChatEffortValueMode =
  | "passthrough"
  | "low_high"
  | "deepseek"
  // OpenRouter effort 鏋氫妇 xhigh|high|medium|low|minimal锛堟棤 max锛宮ax 閽冲埌 xhigh锛?  | "openrouter";

export type CodexChatReasoningOutputFormat =
  | "auto"
  | "reasoning_content"
  | "reasoning"
  | "reasoning_details"
  | "think_tags";

export interface CodexChatReasoning {
  supportsThinking?: boolean;
  supportsEffort?: boolean;
  thinkingParam?: CodexChatThinkingParam;
  effortParam?: CodexChatEffortParam;
  effortValueMode?: CodexChatEffortValueMode;
  // 澹版槑鎬у瓧娈碉細鏍囨敞涓婃父 reasoning 鍥炰紶浣嶇疆銆傚綋鍓嶆彁鍙栭潬绌蜂妇瀛楁锛屾湭璇诲彇姝ゅ€硷紙think_tags 灏氭湭鎺ョ嚎锛夈€?  outputFormat?: CodexChatReasoningOutputFormat;
}

// 渚涘簲鍟嗗厓鏁版嵁锛堝瓧娈靛悕涓庡悗绔竴鑷达紝淇濇寔 snake_case锛?export interface ProviderMeta {
  // 鑷畾涔夌鐐癸細浠?URL 涓洪敭锛屽€间负绔偣淇℃伅
  custom_endpoints?: Record<string, CustomEndpoint>;
  // 鏄惁鍦ㄥ垏鎹?鍚屾鍒?live 鏃跺簲鐢ㄩ€氱敤閰嶇疆鐗囨
  commonConfigEnabled?: boolean;
  // Claude Desktop 3P 閰嶇疆鍐欏叆妯″紡
  claudeDesktopMode?: "direct" | "proxy";
  // Claude Desktop 鏈湴璺敱妯″紡锛欳laude-safe route -> upstream model
  claudeDesktopModelRoutes?: Record<string, ClaudeDesktopModelRoute>;
  // 鐢ㄩ噺鏌ヨ鑴氭湰閰嶇疆
  usage_script?: UsageScript;
  // 璇锋眰鍦板潃绠＄悊锛氭祴閫熷悗鑷姩閫夋嫨鏈€浣崇鐐?  endpointAutoSelect?: boolean;
  // 鏄惁涓哄畼鏂瑰悎浣滀紮浼?  isPartner?: boolean;
  // 鍚堜綔浼欎即淇冮攢 key锛堢敤浜庡悗绔瘑鍒?PackyCode 绛夛級
  partnerPromotionKey?: string;
  // 渚涘簲鍟嗗崟鐙殑妯″瀷娴嬭瘯閰嶇疆
  testConfig?: ProviderTestConfig;
  // 渚涘簲鍟嗘垚鏈€嶇巼
  costMultiplier?: string;
  // 渚涘簲鍟嗚璐规ā寮忔潵婧?  pricingModelSource?: string;
  // API 鏍煎紡锛圕laude / Codex 渚涘簲鍟嗕娇鐢級
  // - "anthropic": 鍘熺敓 Anthropic Messages API 鏍煎紡锛岀洿鎺ラ€忎紶
  // - "openai_chat": OpenAI Chat Completions 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?  // - "openai_responses": OpenAI Responses API 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?  // - "gemini_native": Gemini Native generateContent API 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native";
  // 閫氱敤璁よ瘉缁戝畾
  authBinding?: AuthBinding;
  // Claude 璁よ瘉瀛楁鍚?  apiKeyField?: ClaudeApiKeyField;
  // 鏄惁灏?base_url 瑙嗕负瀹屾暣 API 绔偣锛堜唬鐞嗙洿鎺ヤ娇鐢ㄦ URL锛屼笉鎷兼帴璺緞锛?  isFullUrl?: boolean;
  // Prompt cache key for OpenAI Responses-compatible endpoints (improves cache hit rate)
  promptCacheKey?: string;
  // Codex OAuth FAST mode: injects service_tier="priority" on ChatGPT Codex requests
  codexFastMode?: boolean;
  // Codex Responses -> Chat Completions reasoning capability metadata
  codexChatReasoning?: CodexChatReasoning;
  // 渚涘簲鍟嗙被鍨嬶紙鐢ㄤ簬璇嗗埆 Copilot 绛夌壒娈婁緵搴斿晢锛?  providerType?: string;
  // GitHub Copilot 鍏宠仈璐﹀彿 ID锛堟棫瀛楁锛屼繚鐣欏吋瀹硅鍙栵級
  githubAccountId?: string;
  // 澶氭ā鎬侀檷绾фā鍨嬶細褰撹姹傚寘鍚浘鐗囦笖褰撳墠妯″瀷涓嶆敮鎸佸妯℃€佹椂锛岃嚜鍔ㄥ垏鎹㈠埌姝ゆā鍨?  multimodalFallbackModel?: string;
}

// Skill 鍚屾鏂瑰紡
export type SkillSyncMethod = "auto" | "symlink" | "copy";

// Skill 瀛樺偍浣嶇疆
export type SkillStorageLocation = "cc_switch" | "unified";

// Claude API 鏍煎紡绫诲瀷
// - "anthropic": 鍘熺敓 Anthropic Messages API 鏍煎紡锛岀洿鎺ラ€忎紶
// - "openai_chat": OpenAI Chat Completions 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?// - "openai_responses": OpenAI Responses API 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?// - "gemini_native": Gemini Native generateContent API 鏍煎紡锛岄渶瑕佹牸寮忚浆鎹?export type ClaudeApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native";

// Codex API 鏍煎紡绫诲瀷
// - "openai_responses": OpenAI Responses API 鏍煎紡锛岀洿鎺ラ€忎紶
// - "openai_chat": OpenAI Chat Completions 鏍煎紡锛岄渶瑕佹湰鍦拌矾鐢辫浆鎹?export type CodexApiFormat = "openai_responses" | "openai_chat";

export interface CodexCatalogModel {
  model: string;
  displayName?: string;
  contextWindow?: string | number;
  /** 是否支持多模态输入（图片等） */
  supportsMultimodal?: boolean;
}

// Claude 璁よ瘉瀛楁绫诲瀷
export type ClaudeApiKeyField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";

// 涓婚〉闈㈡樉绀虹殑搴旂敤閰嶇疆
export interface VisibleApps {
  claude: boolean;
  "claude-desktop": boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

// WebDAV 鍚屾鐘舵€?export interface WebDavSyncStatus {
  lastSyncAt?: number | null;
  lastError?: string | null;
  lastErrorSource?: string | null;
  lastRemoteEtag?: string | null;
  lastLocalManifestHash?: string | null;
  lastRemoteManifestHash?: string | null;
}

// WebDAV 鍚屾閰嶇疆
export interface WebDavSyncSettings {
  enabled?: boolean;
  autoSync?: boolean;
  baseUrl?: string;
  username?: string;
  password?: string;
  remoteRoot?: string;
  profile?: string;
  status?: WebDavSyncStatus;
}

export type RemoteSnapshotLayout = "current" | "legacy";

// 杩滅蹇収淇℃伅锛堜笅杞藉墠棰勮锛?export interface RemoteSnapshotInfo {
  deviceName: string;
  createdAt: string;
  snapshotId: string;
  version: number;
  protocolVersion: number;
  dbCompatVersion?: number | null;
  compatible: boolean;
  artifacts: string[];
  layout: RemoteSnapshotLayout;
  remotePath: string;
}

// 搴旂敤璁剧疆绫诲瀷锛堢敤浜庤缃璇濇涓?Tauri API锛?// 瀛樺偍鍦ㄦ湰鍦?~/.cc-switch/settings.json锛屼笉闅忔暟鎹簱鍚屾
export interface Settings {
  // ===== 璁惧绾?UI 璁剧疆 =====
  // 鏄惁鍦ㄧ郴缁熸墭鐩橈紙macOS 鑿滃崟鏍忥級鏄剧ず鍥炬爣
  showInTray: boolean;
  // 鐐瑰嚮鍏抽棴鎸夐挳鏃舵槸鍚︽渶灏忓寲鍒版墭鐩樿€屼笉鏄叧闂簲鐢?  minimizeToTrayOnClose: boolean;
  // 鏄惁鍚敤搴旂敤绾х獥鍙ｆ帶鍒舵寜閽紙鏈€灏忓寲/鏈€澶у寲/鍏抽棴锛?  useAppWindowControls?: boolean;
  // 鍚敤 Claude 鎻掍欢鑱斿姩锛堝啓鍏?~/.claude/config.json 鐨?primaryApiKey锛?  enableClaudePluginIntegration?: boolean;
  // 璺宠繃 Claude Code 鍒濇瀹夎纭锛堝啓鍏?~/.claude.json 鐨?hasCompletedOnboarding锛?  skipClaudeOnboarding?: boolean;
  // 鏄惁寮€鏈鸿嚜鍚?  launchOnStartup?: boolean;
  // 闈欓粯鍚姩锛堢▼搴忓惎鍔ㄦ椂涓嶆樉绀轰富绐楀彛锛?  silentStartup?: boolean;
  // 鏄惁鍚敤涓婚〉闈㈡湰鍦颁唬鐞嗗姛鑳斤紙榛樿鍏抽棴锛?  enableLocalProxy?: boolean;
  // User has confirmed the local proxy first-run notice
  proxyConfirmed?: boolean;
  // User has confirmed the usage query first-run notice
  usageConfirmed?: boolean;
  // User has confirmed the stream check first-run notice
  streamCheckConfirmed?: boolean;
  // Whether to show the failover toggle independently on the main page
  enableFailoverToggle?: boolean;
  // Preserve Codex ChatGPT login in auth.json when switching third-party providers
  preserveCodexOfficialAuthOnSwitch?: boolean;
  // User has confirmed the failover toggle first-run notice
  failoverConfirmed?: boolean;
  // User has confirmed the first-run welcome notice
  firstRunNoticeConfirmed?: boolean;
  // User has confirmed the auto-sync traffic warning
  autoSyncConfirmed?: boolean;
  // User has confirmed the common config first-run notice
  commonConfigConfirmed?: boolean;
  // 棣栭€夎瑷€锛堝彲閫夛紝榛樿涓枃锛?  language?: "en" | "zh" | "zh-TW" | "ja";

  // 涓婚〉闈㈡樉绀虹殑搴旂敤锛堥粯璁ゅ叏閮ㄦ樉绀猴級
  visibleApps?: VisibleApps;

  // ===== 璁惧绾х洰褰曡鐩?=====
  // 瑕嗙洊 Claude Code 閰嶇疆鐩綍锛堝彲閫夛級
  claudeConfigDir?: string;
  // 瑕嗙洊 Codex 閰嶇疆鐩綍锛堝彲閫夛級
  codexConfigDir?: string;
  // 瑕嗙洊 Gemini 閰嶇疆鐩綍锛堝彲閫夛級
  geminiConfigDir?: string;
  // 瑕嗙洊 OpenCode 閰嶇疆鐩綍锛堝彲閫夛級
  opencodeConfigDir?: string;
  // 瑕嗙洊 OpenClaw 閰嶇疆鐩綍锛堝彲閫夛級
  openclawConfigDir?: string;
  // 瑕嗙洊 Hermes 閰嶇疆鐩綍锛堝彲閫夛級
  hermesConfigDir?: string;

  // ===== 褰撳墠渚涘簲鍟?ID锛堣澶囩骇锛?====
  // 褰撳墠 Claude 渚涘簲鍟?ID锛堜紭鍏堜簬鏁版嵁搴?is_current锛?  currentProviderClaude?: string;
  // 褰撳墠 Claude Desktop 渚涘簲鍟?ID锛堜紭鍏堜簬鏁版嵁搴?is_current锛?  currentProviderClaudeDesktop?: string;
  // 褰撳墠 Codex 渚涘簲鍟?ID锛堜紭鍏堜簬鏁版嵁搴?is_current锛?  currentProviderCodex?: string;
  // 褰撳墠 Gemini 渚涘簲鍟?ID锛堜紭鍏堜簬鏁版嵁搴?is_current锛?  currentProviderGemini?: string;

  // ===== Skill 鍚屾璁剧疆 =====
  // Skill 鍚屾鏂瑰紡锛歛uto锛堥粯璁わ紝浼樺厛 symlink锛夈€乻ymlink銆乧opy
  skillSyncMethod?: SkillSyncMethod;
  // Skill 瀛樺偍浣嶇疆锛歝c_switch锛堥粯璁わ級鎴?unified锛垀/.agents/skills/锛?  skillStorageLocation?: SkillStorageLocation;

  // ===== WebDAV v2 鍚屾璁剧疆 =====
  webdavSync?: WebDavSyncSettings;

  // ===== 澶囦唤绛栫暐璁剧疆 =====
  // Auto-backup interval in hours (0=disabled, default 24)
  backupIntervalHours?: number;
  // Maximum backup files to retain (default 10)
  backupRetainCount?: number;

  // ===== 缁堢璁剧疆 =====
  // 棣栭€夌粓绔簲鐢紙鍙€夛紝榛樿浣跨敤绯荤粺榛樿缁堢锛?  // macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "wezterm" | "kaku"
  // Windows: "cmd" | "powershell" | "wt"
  // Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
  preferredTerminal?: string;

  // ===== 鏈満鑷姩杩佺Щ鐘舵€?=====
  localMigrations?: {
    codexThirdPartyHistoryProviderBucketV1?: {
      completedAt: string;
      targetProviderId: string;
      sourceProviderIds?: string[];
      migratedJsonlFiles?: number;
      migratedStateRows?: number;
    };
  };
}

export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string | null;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath?: string;
  resumeCommand?: string;
}

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number;
}

// MCP 鏈嶅姟鍣ㄨ繛鎺ュ弬鏁帮紙瀹芥澗锛氬厑璁告墿灞曞瓧娈碉級
export interface McpServerSpec {
  // 鍙€夛細绀惧尯甯歌 .mcp.json 涓?stdio 閰嶇疆鍙笉鍐?type
  type?: "stdio" | "http" | "sse";
  // stdio 瀛楁
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  // http 鍜?sse 瀛楁
  url?: string;
  headers?: Record<string, string>;
  // 閫氱敤瀛楁
  [key: string]: any;
}

// v3.7.0: MCP 鏈嶅姟鍣ㄥ簲鐢ㄥ惎鐢ㄧ姸鎬?export interface McpApps {
  claude: boolean;
  "claude-desktop"?: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

// MCP 鏈嶅姟鍣ㄦ潯鐩紙v3.7.0 缁熶竴缁撴瀯锛?export interface McpServer {
  id: string;
  name: string;
  server: McpServerSpec;
  apps: McpApps; // v3.7.0: 鏍囪搴旂敤鍒板摢浜涘鎴风
  description?: string;
  tags?: string[];
  homepage?: string;
  docs?: string;
  // 鍏煎鏃у瓧娈碉紙v3.6.x 鍙婁互鍓嶏級
  enabled?: boolean; // 宸插簾寮冿紝v3.7.0 浣跨敤 apps 瀛楁
  source?: string;
  [key: string]: any;
}

// MCP 鏈嶅姟鍣ㄦ槧灏勶紙id -> McpServer锛?export type McpServersMap = Record<string, McpServer>;

// MCP 閰嶇疆鐘舵€?export interface McpStatus {
  userConfigPath: string;
  userConfigExists: boolean;
  serverCount: number;
}

// 鏂帮細鏉ヨ嚜 config.json 鐨?MCP 鍒楄〃鍝嶅簲
export interface McpConfigResponse {
  configPath: string;
  servers: Record<string, McpServer>;
}

// ============================================================================
// 缁熶竴渚涘簲鍟嗭紙Universal Provider锛? 璺ㄥ簲鐢ㄥ叡浜厤缃?// ============================================================================

// 缁熶竴渚涘簲鍟嗙殑搴旂敤鍚敤鐘舵€?export interface UniversalProviderApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
}

// Claude 妯″瀷閰嶇疆
export interface ClaudeModelConfig {
  model?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
}

// Codex 妯″瀷閰嶇疆
export interface CodexModelConfig {
  model?: string;
  reasoningEffort?: string;
}

// Gemini 妯″瀷閰嶇疆
export interface GeminiModelConfig {
  model?: string;
}

// 鍚勫簲鐢ㄧ殑妯″瀷閰嶇疆
export interface UniversalProviderModels {
  claude?: ClaudeModelConfig;
  codex?: CodexModelConfig;
  gemini?: GeminiModelConfig;
}

// 缁熶竴渚涘簲鍟嗭紙璺ㄥ簲鐢ㄥ叡浜厤缃級
export interface UniversalProvider {
  id: string;
  name: string;
  providerType: string; // "newapi" | "custom" 绛?  apps: UniversalProviderApps;
  baseUrl: string;
  apiKey: string;
  models: UniversalProviderModels;
  websiteUrl?: string;
  notes?: string;
  icon?: string;
  iconColor?: string;
  meta?: ProviderMeta;
  createdAt?: number;
  sortIndex?: number;
}

// 缁熶竴渚涘簲鍟嗘槧灏勶紙id -> UniversalProvider锛?export type UniversalProvidersMap = Record<string, UniversalProvider>;

// ============================================================================
// OpenCode 涓撳睘閰嶇疆锛坴3.9.2+锛?// ============================================================================

// OpenCode 妯″瀷閰嶇疆
export interface OpenCodeModel {
  name: string;
  limit?: {
    context?: number;
    output?: number;
  };
  options?: Record<string, unknown>; // 妯″瀷绾у埆棰濆閫夐」锛坧rovider 璺敱绛夛級
  // 鏀寔浠绘剰棰濆瀛楁锛坈ost銆乵odalities銆乼hinking銆乿ariants 绛夛級
  [key: string]: unknown;
}

// OpenCode 渚涘簲鍟嗛€夐」
export interface OpenCodeProviderOptions {
  baseURL?: string;
  apiKey?: string;
  headers?: Record<string, string>;
  // 鏀寔棰濆閫夐」锛坱imeout, setCacheKey 绛夛級
  [key: string]: unknown;
}

// OpenCode 渚涘簲鍟嗛厤缃紙settings_config 缁撴瀯锛?export interface OpenCodeProviderConfig {
  npm: string; // AI SDK 鍖呭悕锛屽 "@ai-sdk/openai-compatible"
  name?: string; // 渚涘簲鍟嗘樉绀哄悕绉?  options: OpenCodeProviderOptions;
  models: Record<string, OpenCodeModel>;
}

// OpenCode MCP 鏈嶅姟鍣ㄩ厤缃紙涓庣粺涓€鏍煎紡涓嶅悓锛?export interface OpenCodeMcpServerSpec {
  type: "local" | "remote";
  // local 绫诲瀷瀛楁
  command?: string[]; // 涓庣粺涓€鏍煎紡涓嶅悓锛氬懡浠ゅ拰鍙傛暟鍚堝苟涓烘暟缁?  environment?: Record<string, string>; // 涓庣粺涓€鏍煎紡涓嶅悓锛氫娇鐢?environment 鑰岄潪 env
  // remote 绫诲瀷瀛楁
  url?: string;
  headers?: Record<string, string>;
  // 閫氱敤瀛楁
  enabled?: boolean;
}

// ============================================================================
// OpenClaw 涓撳睘閰嶇疆锛坴3.11.0+锛?// ============================================================================

// OpenClaw 妯″瀷閰嶇疆
export interface OpenClawModel {
  id: string;
  name: string;
  alias?: string;
  reasoning?: boolean; // 鏄惁鏀寔鎺ㄧ悊妯″紡锛堝 o1銆丏eepSeek R1锛?  input?: string[]; // 鏀寔鐨勮緭鍏ョ被鍨嬶紙濡?["text"]銆乕"text", "image"]锛?  cost?: {
    input: number;
    output: number;
    cacheRead?: number; // 缂撳瓨璇诲彇浠锋牸
    cacheWrite?: number; // 缂撳瓨鍐欏叆浠锋牸
  };
  contextWindow?: number;
  maxTokens?: number; // 鏈€澶ц緭鍑?token 鏁?}

// OpenClaw 榛樿妯″瀷閰嶇疆锛坅gents.defaults.model锛?export interface OpenClawDefaultModel {
  primary: string;
  fallbacks?: string[];
}

// OpenClaw 妯″瀷鐩綍鏉＄洰锛坅gents.defaults.models 涓殑鍊硷級
export interface OpenClawModelCatalogEntry {
  alias?: string;
}

export interface OpenClawHealthWarning {
  code: string;
  message: string;
  path?: string;
}

export interface OpenClawWriteOutcome {
  backupPath?: string;
  warnings: OpenClawHealthWarning[];
}

export type OpenClawToolsProfile = "minimal" | "coding" | "messaging" | "full";

// OpenClaw 渚涘簲鍟嗛厤缃紙settings_config 缁撴瀯锛?// 瀵瑰簲 OpenClaw 鐨?models.providers.<provider-id> 閰嶇疆
export interface OpenClawProviderConfig {
  baseUrl?: string; // API 绔偣
  apiKey?: string; // API 瀵嗛挜
  api?: string; // API 鍗忚绫诲瀷锛堝 "openai-completions"銆?anthropic"锛?  models?: OpenClawModel[]; // 鍙敤妯″瀷鍒楄〃
  headers?: Record<string, string>; // 鑷畾涔夎姹傚ご锛堝 User-Agent锛?  authHeader?: boolean; // 渚涘簲鍟嗚嚜瀹氫箟璁よ瘉寮€鍏筹紙濡?Longcat锛?}

// OpenClaw agents.defaults 瀹屾暣閰嶇疆
export interface OpenClawAgentsDefaults {
  model?: OpenClawDefaultModel;
  models?: Record<string, OpenClawModelCatalogEntry>;
  timeoutSeconds?: number;
  timeout?: number;
  [key: string]: unknown; // preserve unknown fields
}

// OpenClaw env 閰嶇疆锛坥penclaw.json 鐨?env 鑺傜偣锛?export interface OpenClawEnvConfig {
  [key: string]: unknown;
}

// OpenClaw tools 閰嶇疆锛坥penclaw.json 鐨?tools 鑺傜偣锛?export interface OpenClawToolsConfig {
  profile?: OpenClawToolsProfile | string;
  allow?: string[];
  deny?: string[];
  [key: string]: unknown; // preserve unknown fields
}

// ============================================================================
// Hermes Agent 涓撳睘閰嶇疆
// ============================================================================

export interface HermesModelConfig {
  default?: string;
  provider?: string;
  base_url?: string;
  context_length?: number;
  max_tokens?: number;
  [key: string]: unknown;
}

export type HermesMemoryKind = "memory" | "user";

export interface HermesMemoryLimits {
  memory: number;
  user: number;
  memoryEnabled: boolean;
  userEnabled: boolean;
}
