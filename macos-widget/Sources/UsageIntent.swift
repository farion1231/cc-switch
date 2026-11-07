// 「用量」小组件的配置项：在桌面上右键小组件 →「编辑小组件」里选趋势图的样式，各尺寸通用。
// 时间范围不在这里：直接点小组件底栏的范围按钮（小尺寸用 ‹ ›）。文案在 Resources/*.lproj/Localizable.strings。

import AppIntents
import WidgetKit

enum UsageChartOption: String, AppEnum {
    case bars
    case line

    static let typeDisplayRepresentation = TypeDisplayRepresentation(
        name: LocalizedStringResource("usage.chart", defaultValue: "Chart")
    )
    static let caseDisplayRepresentations: [UsageChartOption: DisplayRepresentation] = [
        .bars: DisplayRepresentation(title: LocalizedStringResource("usage.chart.bars", defaultValue: "Bar")),
        .line: DisplayRepresentation(title: LocalizedStringResource("usage.chart.line", defaultValue: "Line")),
    ]

    var kind: UsageChart.Kind {
        switch self {
        case .bars: return .bars
        case .line: return .line
        }
    }
}

struct UsageWidgetIntent: WidgetConfigurationIntent {
    static let title = LocalizedStringResource("usage.intent.title", defaultValue: "Usage")
    static let description = IntentDescription(
        LocalizedStringResource("usage.intent.description", defaultValue: "Choose the chart style. Tap the range buttons on the widget to change the time range.")
    )

    @Parameter(title: LocalizedStringResource("usage.chart", defaultValue: "Chart"), default: .bars)
    var chart: UsageChartOption

    init() {}

    init(chart: UsageChartOption) {
        self.chart = chart
    }
}

struct UsageEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
    let chart: UsageChartOption
}

struct UsageIntentProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> UsageEntry {
        UsageEntry(date: .now, snapshot: .sample, chart: .bars)
    }

    func snapshot(for configuration: UsageWidgetIntent, in context: Context) async -> UsageEntry {
        let snapshot = SnapshotStore.load() ?? (context.isPreview ? .sample : nil)
        return UsageEntry(date: .now, snapshot: snapshot, chart: configuration.chart)
    }

    func timeline(for configuration: UsageWidgetIntent, in context: Context) async -> Timeline<UsageEntry> {
        // 平时由主程序写完快照后通知重读；这里只是兜底，外加到点换成「数据过时」
        let snapshot = SnapshotStore.load()
        var entries = [UsageEntry(date: .now, snapshot: snapshot, chart: configuration.chart)]
        if let snapshot {
            let staleAt = generatedTime(snapshot).addingTimeInterval(staleAfter)
            if staleAt > .now { entries.append(UsageEntry(date: staleAt, snapshot: snapshot, chart: configuration.chart)) }
        }
        return Timeline(entries: entries, policy: .after(.now.addingTimeInterval(SnapshotProvider.reloadInterval)))
    }
}
