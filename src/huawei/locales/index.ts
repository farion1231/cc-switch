import i18n from "@/i18n";
import zh from "./zh.json";
import en from "./en.json";
import ja from "./ja.json";
import zhTw from "./zh-TW.json";

for (const [locale, resources] of [
  ["zh", zh],
  ["en", en],
  ["ja", ja],
  ["zh-TW", zhTw],
] as const) {
  for (const ns of Object.keys(resources)) {
    i18n.addResourceBundle(locale, "translation", { [ns]: resources[ns as keyof typeof resources] }, true, true);
  }
}
