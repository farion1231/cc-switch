// 示例数据：小组件库预览、占位，以及预览程序。

import Foundation

private func q(_ text: String, _ left: Double?, warn: Bool = false) -> QuotaLine {
    QuotaLine(text: text, left: left, warn: warn, resetsAt: nil)
}

private let signedIn = Date.now.timeIntervalSince1970 - 40 * 86_400
private func inHours(_ h: Double) -> String { ISO8601DateFormatter().string(from: .now.addingTimeInterval(h * 3600)) }
private func t(_ name: String, used: Double, resetIn hours: Double? = nil) -> TierRow {
    TierRow(name: name, utilization: used, resetsAt: hours.map(inHours))
}
private func rows(_ tiers: [TierRow], resets: ResetCreditsRow? = nil, credits: Double? = nil) -> AccountQuota {
    .rows(tiers: tiers, resetCredits: resets, creditsBalance: credits, queriedAt: nil)
}

private func account(
    _ service: String, _ serviceName: String, _ login: String,
    inUse: Bool = false, isDefault: Bool = false, needsReauth: Bool = false, users: Int = 0,
    details: AccountDetails = AccountDetails(signedInAt: signedIn), _ quota: AccountQuota = .none
) -> WidgetAccount {
    WidgetAccount(
        service: service, serviceName: serviceName, id: "\(service):\(login)", login: login,
        inUse: inUse, isDefault: isDefault, needsReauth: needsReauth, details: details,
        users: users, quota: quota
    )
}

extension WidgetSnapshot {
    static let sampleAccountsFew: [WidgetAccount] = [
        account("claude", "Claude", "me@work.dev", inUse: true, users: 2,
                details: AccountDetails(signedInAt: signedIn, plan: "max", rateLimitTier: "default_claude_max_20x"),
                rows([t("five_hour", used: 38, resetIn: 2.2), t("seven_day", used: 82, resetIn: 70), t("seven_day_opus", used: 40, resetIn: 70)])),
        account("chatgpt", "ChatGPT", "me@gmail.com", inUse: true, isDefault: true, users: 1,
                rows([t("five_hour", used: 10, resetIn: 3), t("seven_day", used: 29, resetIn: 100)],
                     resets: ResetCreditsRow(count: 2, firstExpiry: inHours(60)), credits: 1250)),
        account("copilot", "Copilot", "octocat", isDefault: true,
                details: AccountDetails(signedInAt: signedIn, domain: "github.com"),
                rows([t("premium", used: 46, resetIn: 200)])),
    ]

    static let sampleAccountsMany: [WidgetAccount] = sampleAccountsFew + [
        account("claude", "Claude", "old-pro@mail.com", needsReauth: true,
                details: AccountDetails(plan: "pro", reauthReason: "claudeExpired")),
        account("chatgpt", "ChatGPT", "plus-2@gmail.com", rows([t("five_hour", used: 92, resetIn: 1), t("seven_day", used: 60, resetIn: 50)])),
        account("xai", "xAI", "grok-heavy", inUse: true, isDefault: true, rows([t("weekly_limit", used: 4, resetIn: 90)])),
        account("xai", "xAI", "grok-alt", .failed(kind: "other", message: "HTTP 429")),
        account("kimi", "Kimi", "Owlbay", inUse: true, isDefault: true,
                details: AccountDetails(region: "mainland-cn", primary: true), .note("noSubscription")),
        account("kimi", "Kimi", "kimi-global", details: AccountDetails(region: "global"),
                rows([t("five_hour", used: 100, resetIn: 0.5), t("weekly_limit", used: 70)])),
        account("agy", "Antigravity", "dev@gmail.com", inUse: true, isDefault: true,
                rows([t("agy_gemini_five_hour", used: 0), t("agy_gemini_weekly", used: 12, resetIn: 40),
                      t("agy_third_party_five_hour", used: 85, resetIn: 1.5), t("agy_third_party_weekly", used: 30, resetIn: 40)])),
        account("agy", "Antigravity", "side@gmail.com", needsReauth: true,
                details: AccountDetails(signedInAt: signedIn, reauthReason: "agyExpired")),
        account("copilot", "Copilot", "work-gh", details: AccountDetails(signedInAt: signedIn, domain: "corp.ghe.com"),
                rows([t("premium", used: 88, resetIn: 200)])),
    ]

    static let sampleUsage: WidgetUsage = {
        let cal = Calendar.current
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        let costs: [Double] = [3.1, 5.4, 0, 7.8, 9.2, 4.4, 2.0, 0, 0, 6.1, 11.3, 8.7, 5.5, 3.9, 12.6,
                               9.9, 4.2, 0, 1.5, 7.7, 10.4, 13.8, 6.6, 5.1, 8.8, 0.9, 0, 9.4, 14.2, 12.34]
        let days = costs.enumerated().map { i, cost -> UsageDay in
            let date = cal.date(byAdding: .day, value: i - (costs.count - 1), to: .now)!
            return UsageDay(date: f.string(from: date), costValue: cost, tokens: Int(cost * 310_000), requests: Int(cost * 42))
        }
        let byApp = [
            UsageByApp(app: "claude", name: "Claude Code", cost: "$7.92", costValue: 7.92, tokens: 2_410_000),
            UsageByApp(app: "codex", name: "Codex", cost: "$3.18", costValue: 3.18, tokens: 1_020_000),
            UsageByApp(app: "gemini", name: "Gemini CLI", cost: "$0.96", costValue: 0.96, tokens: 310_000),
            UsageByApp(app: "opencode", name: "OpenCode", cost: "$0.28", costValue: 0.28, tokens: 86_400),
        ]
        let dayPoints = days.map {
            UsagePoint(start: $0.day.timeIntervalSince1970, costValue: $0.costValue, tokens: $0.tokens, requests: $0.requests)
        }
        let midnight = cal.startOfDay(for: .now).timeIntervalSince1970
        let hours: [UsagePoint] = (0..<24).map { h in
            let cost = [0, 0, 0, 0, 0, 0, 0, 0.2, 0.6, 1.1, 1.4, 0.9, 0.3, 0.8, 1.6, 1.9, 1.2, 0.7, 0.4, 0.5, 0.3, 0.2, 0.1, 0][h]
            return UsagePoint(start: midnight + Double(h) * 3600, costValue: cost, tokens: Int(cost * 310_000), requests: Int(cost * 42))
        }
        func totals(_ cost: Double) -> UsageTotals {
            UsageTotals(cost: String(format: "$%.2f", cost), costValue: cost, tokens: Int(cost * 310_000), requests: Int(cost * 42), cacheHitRate: 0.86)
        }
        func range(_ points: [UsagePoint], _ granularity: String) -> UsageRange {
            UsageRange(totals: totals(points.map(\.costValue).reduce(0, +)), granularity: granularity, series: points, byApp: byApp)
        }
        var usage = WidgetUsage(
            today: UsageTotals(cost: "$12.34", costValue: 12.34, tokens: 3_826_400, requests: 518, cacheHitRate: 0.87),
            last7Days: UsageTotals(cost: "$62.84", costValue: 62.84, tokens: 19_480_000, requests: 2_640, cacheHitRate: 0.84),
            days: days,
            byApp: byApp
        )
        usage.ranges = [
            "today": range(hours, "hour"),
            "1d": range(hours, "hour"),
            "7d": range(Array(dayPoints.suffix(7)), "day"),
            "14d": range(Array(dayPoints.suffix(14)), "day"),
            "30d": range(dayPoints, "day"),
            "all": range(dayPoints, "day"),
        ]
        return usage
    }()

    static let sample = WidgetSnapshot(
        version: 1,
        generatedAt: Date.now.timeIntervalSince1970 - 120,
        apps: [
            WidgetApp(
                app: "claude", name: "Claude Code", current: "Claude Official", modeKind: "direct",
                attention: nil, quotaLines: [q("5h 62%", 62), q("7d 18%", 18, warn: true)], quotaNote: nil
            ),
            WidgetApp(
                app: "codex", name: "Codex", current: "ChatGPT", modeKind: "direct",
                attention: nil, quotaLines: [q("5h 90%", 90), q("7d 71%", 71)], quotaNote: nil
            ),
            WidgetApp(
                app: "gemini", name: "Gemini CLI", current: "Google OAuth", modeKind: "direct",
                attention: nil, quotaLines: [], quotaNote: nil
            ),
        ],
        accounts: sampleAccountsFew,
        usage: sampleUsage
    )

    func with(accounts: [WidgetAccount]) -> WidgetSnapshot {
        var copy = self
        copy.accounts = accounts
        return copy
    }
}
