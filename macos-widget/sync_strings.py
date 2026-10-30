#!/usr/bin/env python3
"""把小组件要用的界面文案从 app 的翻译文件（src/i18n/locales/*.json）同步到
Resources/<lang>.lproj/AppStrings.strings。授权中心小组件的档名、「剩余 x%」、重置次数、
「需要重新登录」等和 app 里一字不差；app 改了文案重新跑一次（build.sh 每次构建都会跑）。

键名保持 app 里的点分路径；i18next 的 {{name}} 占位符原样保留，由小组件替换。
"""
import json
import os

ROOT = os.path.dirname(os.path.abspath(__file__))
LOCALES = os.path.join(ROOT, "..", "src", "i18n", "locales")
# app 的语言 → lproj 名
LANGS = {"en": "en", "zh": "zh-Hans", "zh-TW": "zh-Hant", "ja": "ja", "tr": "tr"}

KEYS = [
    # 档名（TIER_I18N_KEYS）
    *[f"subscription.{k}" for k in (
        "fiveHour", "sevenDay", "sevenDayFable", "sevenDayOpus", "sevenDaySonnet", "thirtyDay",
        "geminiPro", "geminiFlash", "geminiFlashLite",
        "agyGeminiFiveHour", "agyGeminiWeekly", "agyThirdPartyFiveHour", "agyThirdPartyWeekly",
        "monthly", "credits", "copilotPremium", "queryFailed",
    )],
    # 额度行
    "quota.left", "quota.used", "quota.usedUp", "quota.failed", "quota.tierShort",
    "quota.reason.loginExpired", "quota.reason.tokenRefreshPending",
    "quota.resetCredits.label", "quota.resetCredits.value",
    "quota.resetCredits.expiresOn", "quota.resetCredits.noExpiry",
    "quota.credits.label", "quota.credits.usd",
    # 账号行
    "authCenter.default", "authCenter.needsReauth", "authCenter.signedInOn",
    "authCenter.enterpriseServer", "authCenter.codexReauthNote", "authCenter.xaiReauthNote",
    "authCenter.agy.reauthNote", "authCenter.agy.addedOn", "authCenter.agy.current",
    "authCenter.claude.reauthNote", "authCenter.claude.addedOn",
    "authCenter.claude.loginExpiresOn", "authCenter.claude.loginMaybeExpired",
    "kimi.oauth.mainland-cn", "kimi.oauth.global", "kimi.oauth.primaryShort",
    "kimi.oauth.signedOut", "kimi.oauth.noSubscription", "kimi.oauth.current",
    *[f"provider.form.claudeAccount.plan.{k}" for k in (
        "pro", "max", "max5x", "max20x", "team", "teamPremium", "enterprise",
    )],
    "managedAuth.usedBy",
]


def lookup(data, key):
    cur = data
    for part in key.split("."):
        if not isinstance(cur, dict) or part not in cur:
            return None
        cur = cur[part]
    return cur if isinstance(cur, str) else None


def main():
    english = json.load(open(os.path.join(LOCALES, "en.json"), encoding="utf-8"))
    for app_lang, lproj in LANGS.items():
        data = json.load(open(os.path.join(LOCALES, f"{app_lang}.json"), encoding="utf-8"))
        out_dir = os.path.join(ROOT, "Resources", f"{lproj}.lproj")
        os.makedirs(out_dir, exist_ok=True)
        lines = ["/* 由 sync_strings.py 从 src/i18n/locales/%s.json 生成，别手改。 */" % app_lang]
        for key in KEYS:
            value = lookup(data, key) or lookup(english, key)
            if value is None:
                raise SystemExit(f"missing key {key} in {app_lang}.json and en.json")
            lines.append(f"{json.dumps(key, ensure_ascii=False)} = {json.dumps(value, ensure_ascii=False)};")
        with open(os.path.join(out_dir, "AppStrings.strings"), "w", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
