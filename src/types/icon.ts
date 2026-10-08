export interface IconMetadata {
  name: string; // 图标名称（小写，如 "openai"）
  displayName: string; // 显示名称（如 "OpenAI"）
  category: string; // 分类（如 "ai-provider", "cloud", "tool"）
  keywords: string[]; // 搜索关键词
  defaultColor?: string; // 默认颜色
  // "tile"：图标自带实心底（方块 / 圆角块 / 圆形 / 白底），在图标框里铺满，
  // 不按透明 logo 那样缩小居中，否则会变成「框里套一块小方块」
  shape?: "tile";
}

export interface IconPreset {
  [key: string]: IconMetadata;
}
