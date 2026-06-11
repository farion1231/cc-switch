import type { GeminiProviderPreset } from "./geminiProviderPresets";

export const antigravityProviderPresets: GeminiProviderPreset[] = [
  {
    name: "Google Official",
    websiteUrl: "https://ai.google.dev/",
    apiKeyUrl: "https://aistudio.google.com/apikey",
    settingsConfig: {
      env: {},
    },
    description: "Google 官方 Gemini API (OAuth)",
    category: "official",
    partnerPromotionKey: "google-official",
    theme: {
      icon: "antigravity",
      backgroundColor: "#4285F4",
      textColor: "#FFFFFF",
    },
    icon: "antigravity",
  },
];
