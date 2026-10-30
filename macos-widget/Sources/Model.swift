// 小组件读的快照（主程序 src-tauri/src/services/widget_snapshot.rs 写）。
// 新加的段（accounts / usage）都是可选的：旧版主程序写的快照照样能读，对应小组件显示「需要更新」。

import Foundation

struct WidgetSnapshot: Decodable {
    let version: Int
    let generatedAt: TimeInterval
    /// 各应用在用的那家（托盘面板同一份数据）。
    var apps: [WidgetApp]
    /// 授权中心里的全部账号；主程序还没写这一段时为 nil。
    var accounts: [WidgetAccount]? = nil
    /// 用量统计；同上。
    var usage: WidgetUsage? = nil
    /// 设置里的「额度显示」：left 写剩余，used 写已用
    var quotaDisplay: String? = nil

    init(version: Int, generatedAt: TimeInterval, apps: [WidgetApp],
         accounts: [WidgetAccount]? = nil, usage: WidgetUsage? = nil, quotaDisplay: String? = nil) {
        self.version = version
        self.generatedAt = generatedAt
        self.apps = apps
        self.accounts = accounts
        self.usage = usage
        self.quotaDisplay = quotaDisplay
    }

    private enum CodingKeys: String, CodingKey {
        case version, generatedAt, apps, accounts, usage, quotaDisplay
    }

    /// 分段容错：哪一段（或某一个应用 / 账号）读不懂就只丢那一段，别的小组件照常显示；
    /// 以后主程序加了小组件不认识的东西也不至于整份作废。
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decode(Int.self, forKey: .version)
        generatedAt = try c.decode(TimeInterval.self, forKey: .generatedAt)
        apps = (try? c.decode([Lossy<WidgetApp>].self, forKey: .apps))?.compactMap(\.value) ?? []
        accounts = (try? c.decodeIfPresent([Lossy<WidgetAccount>].self, forKey: .accounts))?
            .map { $0.compactMap(\.value) }
        usage = try? c.decodeIfPresent(WidgetUsage.self, forKey: .usage)
        quotaDisplay = try? c.decodeIfPresent(String.self, forKey: .quotaDisplay)
    }

    /// 文件在但整份都读不懂（主程序和小组件版本对不上）：按旧版处理，小组件提示更新。
    static func unreadable() -> WidgetSnapshot {
        WidgetSnapshot(version: 0, generatedAt: Date.now.timeIntervalSince1970, apps: [])
    }
}

/// 读不懂的元素解成 nil，不连累整个数组。
struct Lossy<T: Decodable>: Decodable {
    let value: T?

    init(from decoder: Decoder) throws {
        value = try? T(from: decoder)
    }
}

struct WidgetApp: Decodable, Identifiable {
    let app: String
    let name: String
    let current: String?
    let modeKind: String
    let attention: String?
    let quotaLines: [QuotaLine]
    let quotaNote: String?

    var id: String { app }
}

struct QuotaLine: Decodable, Identifiable, Hashable {
    let text: String
    let left: Double?
    let warn: Bool
    let resetsAt: String?

    var id: String { text }
}

/// 授权中心的一个账号（主程序 `widget_snapshot/accounts.rs`），按渠道带着授权中心那一行要的全部信息。
struct WidgetAccount: Decodable, Identifiable {
    /// 列表用的唯一键：不同渠道的账号 id 可能相同
    var uid: String { "\(service):\(id)" }

    /// claude / copilot / chatgpt / xai / kimi / agy
    let service: String
    let serviceName: String
    let id: String
    /// 账号名（邮箱或用户名），小组件里按隐私内容处理。
    let login: String
    /// 有在用的供应商绑着它（agy / Kimi：它们现在登着的号）。
    let inUse: Bool
    /// 授权中心的「默认」标签（agy：「agy 在用」，Kimi：「Kimi 在用」）
    let isDefault: Bool
    let needsReauth: Bool
    let details: AccountDetails
    /// 绑着它的供应商数
    let users: Int
    let quota: AccountQuota

    /// 要处理：要重新登录，或额度没查到
    var needsAction: Bool {
        if needsReauth { return true }
        if case .failed = quota { return true }
        return false
    }

    /// 最紧的一档还剩多少（排序用）；没有额度条的算 100。
    var minLeft: Double {
        guard case let .rows(tiers, _, _, _) = quota, !tiers.isEmpty else { return 100 }
        return tiers.map(\.left).min() ?? 100
    }
}

struct AccountDetails: Decodable {
    var signedInAt: TimeInterval? = nil
    var domain: String? = nil
    var plan: String? = nil
    var rateLimitTier: String? = nil
    /// 毫秒
    var loginExpiresAt: Double? = nil
    var region: String? = nil
    var primary: Bool = false
    /// codexIdToken / xaiExpired / agyExpired / claudeExpired / claudeMaybeExpired / kimiSignedOut
    var reauthReason: String? = nil
    var error: String? = nil
}

struct TierRow: Decodable, Hashable {
    let name: String
    /// 已用 0–100
    let utilization: Double
    let resetsAt: String?

    var left: Double { max(0, (100 - utilization).rounded()) }
}

struct ResetCreditsRow: Decodable, Hashable {
    let count: Int
    let firstExpiry: String?
    /// 每一次的到期时间（不过期的为 nil）；旧版主程序没有
    var expiresAt: [String?]? = nil

    /// 按当前时间还剩几次、最早哪次到期：查询之后又到期的不算（照 quotaRules.ts 渲染时现滤）
    func remaining(at now: Date) -> (count: Int, firstExpiry: Date?) {
        guard let expiresAt else { return (count, parseISO(firstExpiry)) }
        let left = expiresAt.filter { at in
            guard let date = parseISO(at) else { return true }
            return date > now
        }
        return (left.count, left.first.flatMap { parseISO($0) })
    }
}

/// 授权中心账号行右边的额度列。
enum AccountQuota: Decodable {
    case none
    case rows(tiers: [TierRow], resetCredits: ResetCreditsRow?, creditsBalance: Double?, queriedAt: Double?)
    case failed(kind: String, message: String?)
    case note(String)

    private enum CodingKeys: String, CodingKey {
        case state, tiers, resetCredits, creditsBalance, queriedAt, kind, message, note
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .state) {
        case "rows":
            self = .rows(
                tiers: try c.decode([TierRow].self, forKey: .tiers),
                resetCredits: try c.decodeIfPresent(ResetCreditsRow.self, forKey: .resetCredits),
                creditsBalance: try c.decodeIfPresent(Double.self, forKey: .creditsBalance),
                queriedAt: try c.decodeIfPresent(Double.self, forKey: .queriedAt)
            )
        case "failed":
            self = .failed(
                kind: try c.decode(String.self, forKey: .kind),
                message: try c.decodeIfPresent(String.self, forKey: .message)
            )
        case "note":
            self = .note(try c.decode(String.self, forKey: .note))
        default:
            self = .none
        }
    }
}

struct WidgetUsage: Decodable {
    /// 金额已带单位，如 "$12.34"
    let today: UsageTotals
    let last7Days: UsageTotals
    /// 最近 30 天每天一条，旧 → 新，没有用量的日子也有（全 0）。
    let days: [UsageDay]
    /// 今天按应用拆分，按花费从高到低。
    let byApp: [UsageByApp]
    /// 用量页的六个范围（键：today / 1d / 7d / 14d / 30d / all）；旧版主程序没有这一段。
    var ranges: [String: UsageRange]? = nil
}

struct UsageRange: Decodable {
    let totals: UsageTotals
    /// hour / day / week / month
    let granularity: String
    let series: [UsagePoint]
    let byApp: [UsageByApp]
}

struct UsagePoint: Decodable, Identifiable {
    /// 这一格的起点（Unix 秒）
    let start: TimeInterval
    let costValue: Double
    let tokens: Int
    let requests: Int

    var id: TimeInterval { start }
    var date: Date { Date(timeIntervalSince1970: start) }
}

struct UsageTotals: Decodable {
    let cost: String
    let costValue: Double
    let tokens: Int
    let requests: Int
    /// 0–1
    let cacheHitRate: Double
}

struct UsageDay: Decodable, Identifiable {
    /// YYYY-MM-DD（本地）
    let date: String
    let costValue: Double
    let tokens: Int
    let requests: Int

    var id: String { date }
    var day: Date {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.date(from: date) ?? .now
    }
}

struct UsageByApp: Decodable, Identifiable {
    let app: String
    let name: String
    let cost: String
    let costValue: Double
    let tokens: Int

    var id: String { app }
}

enum SnapshotStore {
    static let fileName = "widget-snapshot.json"

    /// 打包脚本按 Team ID 写进 Info.plist，主程序也从这里读，两边保证一致。
    static var appGroup: String? {
        Bundle.main.object(forInfoDictionaryKey: "CCSwitchAppGroup") as? String
    }

    static func load() -> WidgetSnapshot? {
        guard let group = appGroup,
              let dir = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group),
              let data = try? Data(contentsOf: dir.appendingPathComponent(fileName))
        else { return nil }
        return (try? JSONDecoder().decode(WidgetSnapshot.self, from: data)) ?? .unreadable()
    }
}

/// 账号很多时谁先上：要处理的（要重新登录 / 没查到）→ 额度最紧的 → 在用的 → 其余按服务。
func prioritized(_ accounts: [WidgetAccount]) -> [WidgetAccount] {
    accounts.sorted { a, b in
        if a.needsAction != b.needsAction { return a.needsAction }
        if a.minLeft != b.minLeft { return a.minLeft < b.minLeft }
        if a.inUse != b.inUse { return a.inUse }
        return (a.serviceName, a.login) < (b.serviceName, b.login)
    }
}
