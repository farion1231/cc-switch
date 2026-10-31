// 「用量」小组件的配置项：在桌面上右键小组件 →「编辑小组件」里选时间范围和样式。
// 文案在 Resources/*.lproj/Localizable.strings；时间范围和用量页的六个按钮同名同口径。

import AppIntents
import WidgetKit

enum UsageRangeOption: String, AppEnum {
    case today
    case last24h = "1d"
    case last7d = "7d"
    case last14d = "14d"
    case last30d = "30d"
    case all

    static let typeDisplayRepresentation = TypeDisplayRepresentation(
        name: LocalizedStringResource("usage.range", defaultValue: "Time Range")
    )
    static let caseDisplayRepresentations: [UsageRangeOption: DisplayRepresentation] = [
        .today: DisplayRepresentation(title: LocalizedStringResource("usage.range.today", defaultValue: "Today")),
        .last24h: DisplayRepresentation(title: LocalizedStringResource("usage.range.1d", defaultValue: "Last 24 hours")),
        .last7d: DisplayRepresentation(title: LocalizedStringResource("usage.range.7d", defaultValue: "Last 7 days")),
        .last14d: DisplayRepresentation(title: LocalizedStringResource("usage.range.14d", defaultValue: "Last 14 days")),
        .last30d: DisplayRepresentation(title: LocalizedStringResource("usage.range.30d", defaultValue: "Last 30 days")),
        .all: DisplayRepresentation(title: LocalizedStringResource("usage.range.all", defaultValue: "All")),
    ]

    /// 小组件标题里写的范围名。
    var title: String {
        switch self {
        case .today: return widgetString("usage.range.today", "Today")
        case .last24h: return widgetString("usage.range.1d", "Last 24 hours")
        case .last7d: return widgetString("usage.range.7d", "Last 7 days")
        case .last14d: return widgetString("usage.range.14d", "Last 14 days")
        case .last30d: return widgetString("usage.range.30d", "Last 30 days")
        case .all: return widgetString("usage.range.all", "All")
        }
    }
}

enum UsageStyleOption: String, AppEnum {
    /// 大数字 + 明细 + 趋势（默认）
    case overview
    /// 以趋势图为主
    case trend
    /// 按应用拆分
    case apps
    /// 花费 / Token / 请求 / 缓存命中四个数
    case metrics

    static let typeDisplayRepresentation = TypeDisplayRepresentation(
        name: LocalizedStringResource("usage.style", defaultValue: "Style")
    )
    static let caseDisplayRepresentations: [UsageStyleOption: DisplayRepresentation] = [
        .overview: DisplayRepresentation(title: LocalizedStringResource("usage.style.overview", defaultValue: "Overview")),
        .trend: DisplayRepresentation(title: LocalizedStringResource("usage.style.trend", defaultValue: "Trend")),
        .apps: DisplayRepresentation(title: LocalizedStringResource("usage.style.apps", defaultValue: "By App")),
        .metrics: DisplayRepresentation(title: LocalizedStringResource("usage.style.metrics", defaultValue: "Metrics")),
    ]
}

struct UsageWidgetIntent: WidgetConfigurationIntent {
    static let title = LocalizedStringResource("usage.intent.title", defaultValue: "Usage")
    static let description = IntentDescription(
        LocalizedStringResource("usage.intent.description", defaultValue: "Choose the time range and style.")
    )

    @Parameter(title: LocalizedStringResource("usage.range", defaultValue: "Time Range"), default: .today)
    var range: UsageRangeOption

    @Parameter(title: LocalizedStringResource("usage.style", defaultValue: "Style"), default: .overview)
    var style: UsageStyleOption

    init() {}

    init(range: UsageRangeOption, style: UsageStyleOption) {
        self.range = range
        self.style = style
    }
}

struct UsageEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
    let range: UsageRangeOption
    let style: UsageStyleOption
}

struct UsageIntentProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> UsageEntry {
        UsageEntry(date: .now, snapshot: .sample, range: .today, style: .overview)
    }

    func snapshot(for configuration: UsageWidgetIntent, in context: Context) async -> UsageEntry {
        let snapshot = SnapshotStore.load() ?? (context.isPreview ? .sample : nil)
        return UsageEntry(date: .now, snapshot: snapshot, range: configuration.range, style: configuration.style)
    }

    func timeline(for configuration: UsageWidgetIntent, in context: Context) async -> Timeline<UsageEntry> {
        // 平时由主程序写完快照后通知重读；这里只是兜底，外加到点换成「数据过时」
        let snapshot = SnapshotStore.load()
        let make = { (date: Date) in
            UsageEntry(date: date, snapshot: snapshot, range: configuration.range, style: configuration.style)
        }
        var entries = [make(.now)]
        if let snapshot {
            let staleAt = generatedTime(snapshot).addingTimeInterval(staleAfter)
            if staleAt > .now { entries.append(make(staleAt)) }
        }
        return Timeline(entries: entries, policy: .after(.now.addingTimeInterval(SnapshotProvider.reloadInterval)))
    }
}
