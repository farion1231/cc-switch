/**
 * 根据供应商名称智能推断图标配置
 */

const iconMappings = {
  // AI 服务商
  claude: { icon: "claude", iconColor: "#D4915D" },
  anthropic: { icon: "anthropic", iconColor: "#D4915D" },
  deepseek: { icon: "deepseek", iconColor: "#1E88E5" },
  zhipu: { icon: "zhipu", iconColor: "#0F62FE" },
  glm: { icon: "zhipu", iconColor: "#0F62FE" },
  qwen: { icon: "qwen", iconColor: "#FF6A00" },
  alibaba: { icon: "alibaba", iconColor: "#FF6A00" },
  aliyun: { icon: "alibaba", iconColor: "#FF6A00" },
  kimi: { icon: "kimi", iconColor: "#6366F1" },
  moonshot: { icon: "moonshot", iconColor: "#6366F1" },
  baidu: { icon: "baidu", iconColor: "#2932E1" },
  tencent: { icon: "tencent", iconColor: "#00A4FF" },
  hunyuan: { icon: "hunyuan", iconColor: "#00A4FF" },
  minimax: { icon: "minimax", iconColor: "#FF6B6B" },
  google: { icon: "google", iconColor: "#4285F4" },
  meta: { icon: "meta", iconColor: "#0081FB" },
  mistral: { icon: "mistral", iconColor: "#FF7000" },
  cohere: { icon: "cohere", iconColor: "#39594D" },
  perplexity: { icon: "perplexity", iconColor: "#20808D" },
  huggingface: { icon: "huggingface", iconColor: "#FFD21E" },

  // 云平台
  aws: { icon: "aws", iconColor: "#FF9900" },
  azure: { icon: "azure", iconColor: "#0078D4" },
  huawei: { icon: "huawei", iconColor: "#FF0000" },
  cloudflare: { icon: "cloudflare", iconColor: "#F38020" },
};

/**
 * 根据预设名称推断图标
 */
export function inferIconForPreset(presetName: string): {
  icon?: string;
  iconColor?: string;
} {
  const nameLower = presetName.toLowerCase();

  // 精确匹配或模糊匹配
  for (const [key, config] of Object.entries(iconMappings)) {
    if (nameLower.includes(key)) {
      return config;
    }
  }

  return {};
}

/**
 * 批量为预设添加图标配置
 */
export function addIconsToPresets<
  T extends { name: string; icon?: string; iconColor?: string },
>(presets: T[]): T[] {
  return presets.map((preset) => {
    // 如果已经配置了图标，则保留原配置
    if (preset.icon) {
      return preset;
    }

    // 否则根据名称推断
    const inferred = inferIconForPreset(preset.name);
    return {
      ...preset,
      ...inferred,
    };
  });
}
