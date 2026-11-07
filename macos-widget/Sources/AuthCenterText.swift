// 授权中心小组件的文案和排版规则，照 app 的授权中心做：
// - 文案取 Resources/*.lproj/AppStrings.strings（sync_strings.py 从 app 的翻译文件生成，一字不差）
// - 档名 = SubscriptionQuotaFooter.tsx 的 TIER_I18N_KEYS；数值、颜色、倒计时 = quota/quotaRules.ts
// - 第二行 = 各渠道 *Section.tsx 拼的 details

import Foundation
import SwiftUI

// MARK: - 取文案

/// app 的文案（键同 src/i18n/locales/*.json 的点分路径），`{{name}}` 占位符按 args 替换。
func T(_ key: String, _ args: [String: String] = [:]) -> String {
    var text = appStringsBundle.localizedString(forKey: key, value: key, table: "AppStrings")
    for (name, value) in args {
        text = text.replacingOccurrences(of: "{{\(name)}}", with: value)
    }
    return text
}

/// 小组件自己的文案（Localizable.strings：「用量」的范围名、样式名），取法同 `T`。
func widgetString(_ key: String, _ fallback: String) -> String {
    appStringsBundle.localizedString(forKey: key, value: fallback, table: "Localizable")
}

/// 扩展里就是自己的 bundle；预览程序没有 lproj，从 CCSW_RESOURCES 指的目录按系统语言挑一份。
private let appStringsBundle: Bundle = {
    #if CC_SWITCH_WIDGET_PREVIEW
    if let dir = ProcessInfo.processInfo.environment["CCSW_RESOURCES"] {
        let available = (try? FileManager.default.contentsOfDirectory(atPath: dir))?
            .filter { $0.hasSuffix(".lproj") }
            .map { String($0.dropLast(".lproj".count)) } ?? []
        let lang = Bundle.preferredLocalizations(from: available, forPreferences: Locale.preferredLanguages).first ?? "en"
        if let bundle = Bundle(path: "\(dir)/\(lang).lproj") { return bundle }
    }
    #endif
    return .main
}()

/// 写日期用的语言：和文案同一种（扩展取 bundle 选中的 lproj，预览取 CCSW_RESOURCES 里挑中的那份）。
let widgetLocale: Locale = {
    let path = appStringsBundle.bundlePath
    if path.hasSuffix(".lproj") {
        return Locale(identifier: String((path as NSString).lastPathComponent.dropLast(".lproj".count)))
    }
    let lang = Bundle.main.preferredLocalizations.first ?? Locale.preferredLanguages.first ?? "en"
    return Locale(identifier: lang)
}()

// MARK: - 额度行

/// 档名（TIER_I18N_KEYS）；不认识的档不显示，和授权中心一致。
private let tierKeys: [String: String] = [
    "five_hour": "subscription.fiveHour",
    "seven_day": "subscription.sevenDay",
    "seven_day_fable": "subscription.sevenDayFable",
    "seven_day_opus": "subscription.sevenDayOpus",
    "seven_day_sonnet": "subscription.sevenDaySonnet",
    "30_day": "subscription.thirtyDay",
    "gemini_pro": "subscription.geminiPro",
    "gemini_flash": "subscription.geminiFlash",
    "gemini_flash_lite": "subscription.geminiFlashLite",
    "agy_gemini_five_hour": "subscription.agyGeminiFiveHour",
    "agy_gemini_weekly": "subscription.agyGeminiWeekly",
    "agy_third_party_five_hour": "subscription.agyThirdPartyFiveHour",
    "agy_third_party_weekly": "subscription.agyThirdPartyWeekly",
    "weekly_limit": "subscription.sevenDay",
    "monthly": "subscription.monthly",
    "credits": "subscription.credits",
    "premium": "subscription.copilotPremium",
]

enum QuotaTone {
    case normal, warning, danger

    /// quotaRules.ts 的 toneFor：用完 danger，不到 20% warning
    init(left: Double) {
        if left <= 0 { self = .danger } else if left < 20 { self = .warning } else { self = .normal }
    }

    var color: Color {
        switch self {
        case .normal: return .accentColor
        case .warning: return .orange
        case .danger: return .red
        }
    }
}

/// 授权中心额度列的一行：档名 | 条（或一句说明）| 数值 | 倒计时
struct AuthQuotaRow: Identifiable {
    let id: String
    let label: String
    /// 条的长度 0–100；没有条（重置次数、Credits）时为空，改写 caption
    let bar: Double?
    let caption: String?
    let value: String
    /// 中尺寸放不下「剩余」：卡片上的短写法「5 小时 62%」（quota.tierShort）
    let short: String
    /// 剩余百分比（挑最紧的档用）；重置次数、Credits 没有
    var left: Double? = nil
    let tone: QuotaTone
    let resetsAt: Date?
}

private let iso = ISO8601DateFormatter()
private let isoFractional: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return f
}()

func parseISO(_ text: String?) -> Date? {
    guard let text else { return nil }
    return iso.date(from: text) ?? isoFractional.date(from: text)
}

/// 「9月12日」；不是今年的带年份（accountDetails.ts 的 signedInDate、quotaRules.ts 的 shortDate）
func shortDate(_ date: Date) -> String {
    let sameYear = Calendar.current.component(.year, from: date) == Calendar.current.component(.year, from: .now)
    let f = DateFormatter()
    f.locale = widgetLocale
    f.setLocalizedDateFormatFromTemplate(sameYear ? "MMMd" : "yMMMd")
    return f.string(from: date)
}

/// 「2d3h」「2h13m」「13m」；已经过了不写（quotaRules.ts 的 countdownStr）
func countdown(_ date: Date?, now: Date) -> String? {
    guard let date else { return nil }
    let diff = date.timeIntervalSince(now)
    guard diff > 0 else { return nil }
    let hours = Int(diff / 3600)
    let minutes = Int(diff.truncatingRemainder(dividingBy: 3600) / 60)
    if hours > 24 { return "\(hours / 24)d\(hours % 24)h" }
    if hours > 0 { return "\(hours)h\(minutes)m" }
    return "\(minutes)m"
}

/// 一份查询成功的额度 → 各行（quotaRows：各档，再加存下的重置次数和 Credits 余额）
/// 放不下时先留哪几行：越紧越先（用完 → 不到 20% → 剩余少的），重置次数、Credits 排后；
/// 返回保留下来的行，顺序照原来的。
func tightestRows(_ rows: [AuthQuotaRow], limit: Int) -> [AuthQuotaRow] {
    guard rows.count > limit else { return rows }
    let keep = Set(
        rows.enumerated()
            .sorted { ($0.element.left ?? 1000, $0.offset) < ($1.element.left ?? 1000, $1.offset) }
            .prefix(limit)
            .map(\.element.id)
    )
    return rows.filter { keep.contains($0.id) }
}

func authQuotaRows(
    tiers: [TierRow], resetCredits: ResetCreditsRow?, creditsBalance: Double?,
    displayUsed: Bool, now: Date
) -> [AuthQuotaRow] {
    var rows: [AuthQuotaRow] = tiers.compactMap { tier in
        guard let key = tierKeys[tier.name] else { return nil }
        let left = tier.left
        // 用完时两种模式都写「已用完」（quotaRules.ts 的 tierLine）
        let value: String
        if left <= 0 {
            value = T("quota.usedUp")
        } else if displayUsed {
            value = T("quota.used", ["value": "\(Int(100 - left))"])
        } else {
            value = T("quota.left", ["value": "\(Int(left))"])
        }
        let label = T(key)
        let shown = displayUsed ? 100 - left : left
        return AuthQuotaRow(
            id: tier.name, label: label,
            bar: shown, caption: nil,
            value: value,
            short: T("quota.tierShort", ["label": label, "value": "\(Int(shown))"]),
            left: left,
            tone: QuotaTone(left: left), resetsAt: parseISO(tier.resetsAt)
        )
    }
    // 一档都没有时重置次数、余额也不单独出来
    guard !rows.isEmpty else { return rows }
    if let credits = resetCredits, credits.remaining(at: now).count > 0 {
        let (count, expiry) = credits.remaining(at: now)
        let soon = expiry.map { $0.timeIntervalSince(now) < 3 * 24 * 3600 } ?? false
        rows.append(AuthQuotaRow(
            id: "reset_credits", label: T("quota.resetCredits.label"), bar: nil,
            caption: expiry.map { T("quota.resetCredits.expiresOn", ["date": shortDate($0)]) }
                ?? T("quota.resetCredits.noExpiry"),
            value: T("quota.resetCredits.value", ["count": "\(count)"]),
            short: T("quota.resetCredits.value", ["count": "\(count)"]),
            tone: soon ? .warning : .normal, resetsAt: nil
        ))
    }
    if let balance = creditsBalance, balance > 0 {
        // 1 Credit = $0.04（quotaRules.ts 的 CODEX_USD_PER_CREDIT），美元取整
        let usd = Int((balance * 0.04).rounded())
        let dollars = usd >= 1 ? "$\(usd)" : "<$1"
        rows.append(AuthQuotaRow(
            id: "credits_balance", label: T("quota.credits.label"), bar: nil,
            caption: balance.formatted(.number.precision(.fractionLength(0...2))),
            value: T("quota.credits.usd", ["usd": dollars]),
            short: dollars,
            tone: .normal, resetsAt: nil
        ))
    }
    return rows
}

/// 额度查询失败的原因（quotaFailureReason）
func quotaFailureText(kind: String, message: String?) -> String {
    switch kind {
    case "loginExpired": return T("quota.reason.loginExpired")
    case "tokenRefreshPending": return T("quota.reason.tokenRefreshPending")
    default: return message ?? T("subscription.queryFailed")
    }
}

// MARK: - 账号行

/// 「默认」标签的文案：agy、Kimi 写它们自己现在登着的号
func defaultLabel(_ account: WidgetAccount) -> String {
    switch account.service {
    case "agy": return T("authCenter.agy.current")
    case "kimi": return T("kimi.oauth.current")
    default: return T("authCenter.default")
    }
}

/// Claude 套餐（lib/claudePlan.ts）
func claudePlanLabel(_ plan: String?, _ tier: String?) -> String? {
    guard let type = plan?.trimmingCharacters(in: .whitespaces).lowercased(), !type.isEmpty else { return nil }
    let key = "provider.form.claudeAccount.plan"
    switch type {
    case "pro": return T("\(key).pro")
    case "max":
        if tier == "default_claude_max_20x" { return T("\(key).max20x") }
        if tier == "default_claude_max_5x" { return T("\(key).max5x") }
        return T("\(key).max")
    case "team": return tier == "default_claude_max_5x" ? T("\(key).teamPremium") : T("\(key).team")
    case "enterprise": return T("\(key).enterprise")
    default: return nil
    }
}

/// 账号第二行，照各渠道 *Section.tsx 拼：「Max 20x · 9月12日添加」「github.com · 9月12日登录」……
/// 末尾再加「N 个供应商在用」。
func accountDetailsLine(_ account: WidgetAccount, now: Date) -> String {
    let d = account.details
    let date = d.signedInAt.map { shortDate(Date(timeIntervalSince1970: $0)) }
    var parts: [String] = []
    switch account.service {
    case "claude":
        if let plan = claudePlanLabel(d.plan, d.rateLimitTier) { parts.append(plan) }
        switch d.reauthReason {
        case "claudeExpired": parts.append(T("authCenter.claude.reauthNote"))
        case "claudeMaybeExpired": parts.append(T("authCenter.claude.loginMaybeExpired"))
        default:
            if let date { parts.append(T("authCenter.claude.addedOn", ["date": date])) }
            // 七天内过期才提醒
            if let ms = d.loginExpiresAt {
                let expires = Date(timeIntervalSince1970: ms / 1000)
                let left = expires.timeIntervalSince(now)
                if left > 0, left <= 7 * 24 * 3600 {
                    parts.append(T("authCenter.claude.loginExpiresOn", ["date": shortDate(expires)]))
                }
            }
        }
    case "copilot":
        if let domain = d.domain {
            parts.append(domain)
            if domain != "github.com" { parts.append(T("authCenter.enterpriseServer")) }
        }
        if let date { parts.append(T("authCenter.signedInOn", ["date": date])) }
    case "chatgpt":
        if d.reauthReason == "codexIdToken" {
            parts.append(T("authCenter.codexReauthNote"))
        } else if let date {
            parts.append(T("authCenter.signedInOn", ["date": date]))
        }
    case "xai":
        if d.reauthReason == "xaiExpired" {
            parts.append(T("authCenter.xaiReauthNote"))
        } else if let date {
            parts.append(T("authCenter.signedInOn", ["date": date]))
        }
    case "agy":
        if d.reauthReason == "agyExpired" {
            parts.append(T("authCenter.agy.reauthNote"))
        } else if let date {
            parts.append(T("authCenter.agy.addedOn", ["date": date]))
        }
    case "dsh":
        // DshAccount.tsx：登录失效写原因；否则有名字时写联系方式，再写「Web 与 Desktop 共用」
        if d.reauthReason == "dshExpired" {
            parts.append(T("dsh.account.expired"))
        } else {
            if let contact = d.contact { parts.append(contact) }
            parts.append(T("dsh.account.shared"))
        }
    case "kimi":
        if d.reauthReason == "kimiSignedOut" {
            parts.append(d.error ?? T("kimi.oauth.signedOut"))
        } else {
            if let region = d.region { parts.append(T("kimi.oauth.\(region)")) }
            if d.primary { parts.append(T("kimi.oauth.primaryShort")) }
        }
    default:
        break
    }
    if account.users > 0 {
        parts.append(T("managedAuth.usedBy", ["count": "\(account.users)"]))
    }
    return parts.joined(separator: " · ")
}
