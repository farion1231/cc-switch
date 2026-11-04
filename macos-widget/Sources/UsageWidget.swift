// 「用量」：用量统计页同一份数据。按时间范围翻页（当天 / 近 24 小时 / 近 7、14、30 天 / 全部，
// 和用量页的六个按钮同名同口径），底栏左边写现在是哪个范围。四种尺寸各一种排法：
// - 小：大数字花费 + Token / 请求 + 迷你柱状图
// - 中：左边花费和三个数，右边趋势图
// - 大：再加按应用拆分
// - 特大：左边花费 + 趋势图，右边三个数 + 按应用排行

import Charts
import SwiftUI
import WidgetKit

enum UsageRangeOption: String, CaseIterable {
    case today
    case last24h = "1d"
    case last7d = "7d"
    case last14d = "14d"
    case last30d = "30d"
    case all

    /// 底栏里写的范围名（Localizable.strings）
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

func compactNumber(_ value: Int) -> String {
    let v = Double(value)
    switch v {
    case 1_000_000_000...: return String(format: "%.1fB", v / 1_000_000_000)
    case 1_000_000...: return String(format: "%.1fM", v / 1_000_000)
    case 1_000...: return String(format: "%.1fK", v / 1_000)
    default: return "\(value)"
    }
}

// MARK: - 部件

/// 趋势图。柱状（概览）或面积（趋势样式）；横轴按粒度写小时 / 星期 / 日期 / 月份。
struct UsageChart: View {
    enum Kind { case bars, area }

    let points: [UsagePoint]
    let granularity: String
    var kind: Kind = .bars
    var showAxis = false
    @Environment(\.widgetRenderingMode) private var renderingMode

    private var unit: Calendar.Component {
        switch granularity {
        case "hour": return .hour
        case "week": return .weekOfYear
        case "month": return .month
        default: return .day
        }
    }

    private var fullColor: Bool { renderingMode == .fullColor }

    var body: some View {
        let last = points.last?.start
        Chart(points) { point in
            switch kind {
            case .bars:
                BarMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .cornerRadius(2)
                .foregroundStyle(
                    fullColor
                        ? AnyShapeStyle(point.start == last ? Color.accentColor.gradient : Color.accentColor.opacity(0.45).gradient)
                        : AnyShapeStyle(.primary)
                )
            case .area:
                AreaMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .interpolationMethod(.monotone)
                .foregroundStyle(
                    fullColor
                        ? AnyShapeStyle(LinearGradient(colors: [Color.accentColor.opacity(0.45), Color.accentColor.opacity(0.04)], startPoint: .top, endPoint: .bottom))
                        : AnyShapeStyle(.primary.opacity(0.3))
                )
                LineMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .interpolationMethod(.monotone)
                .lineStyle(StrokeStyle(lineWidth: 1.6))
                .foregroundStyle(fullColor ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(.primary))
            }
        }
        .chartYAxis(.hidden)
        .chartXAxis {
            if showAxis { axis }
        }
        .widgetAccentable()
    }

    /// 不画网格线，玻璃上太乱；标签数量控制在 4–7 个。
    @AxisContentBuilder
    private var axis: some AxisContent {
        switch granularity {
        case "hour":
            AxisMarks(preset: .aligned, values: .stride(by: .hour, count: 6)) { _ in
                AxisValueLabel(format: .dateTime.hour())
            }
        case "month":
            AxisMarks(preset: .aligned, values: .stride(by: .month, count: max(1, points.count / 5))) { _ in
                AxisValueLabel(format: .dateTime.year(.twoDigits).month(.defaultDigits))
            }
        case "week":
            AxisMarks(preset: .aligned, values: .stride(by: .weekOfYear, count: max(1, points.count / 5))) { _ in
                AxisValueLabel(format: .dateTime.month(.defaultDigits).day())
            }
        default:
            if points.count <= 7 {
                AxisMarks(preset: .aligned, values: .stride(by: .day)) { _ in
                    AxisValueLabel(format: .dateTime.weekday(.narrow), centered: true)
                }
            } else {
                AxisMarks(preset: .aligned, values: .stride(by: .day, count: points.count <= 14 ? 3 : 7)) { _ in
                    AxisValueLabel(format: .dateTime.month(.defaultDigits).day())
                }
            }
        }
    }
}

struct Metric: View {
    let value: String
    let label: String
    var size: CGFloat = 13

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(value)
                .font(.system(size: size, weight: .semibold, design: .rounded).monospacedDigit())
                .lineLimit(1)
                .minimumScaleFactor(0.6)
            Text(label)
                .font(.system(size: 9))
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

/// 按应用拆分的一行：名字、比例条、金额。
struct AppShareRow: View {
    let app: UsageByApp
    let maxCost: Double
    var nameWidth: CGFloat = 92

    var body: some View {
        HStack(spacing: 8) {
            BrandLogo(icon: appIcon(app.app), fallback: app.name, size: 15)
            Text(app.name)
                .font(.system(size: 11, weight: .medium))
                .lineLimit(1)
                .frame(width: nameWidth, alignment: .leading)
            GeometryReader { geo in
                Capsule()
                    .fill(Color.accentColor.gradient)
                    .frame(width: max(4, geo.size.width * app.costValue / max(maxCost, 0.0001)))
                    .widgetAccentable()
            }
            .frame(height: 6)
            // 金额不换行、不截断：按自身宽度排，比例条让位（「全部」里常见 $24,535.49 这种长金额）
            Text(app.cost)
                .font(.system(size: 10, weight: .medium).monospacedDigit())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .fixedSize()
                .layoutPriority(1)
        }
    }
}

// MARK: - 小组件视图

struct UsageWidgetView: View {
    let entry: SnapshotEntry
    /// 预览程序用：环境里的 widgetFamily 只读，没法从外面设。
    var familyOverride: WidgetFamily? = nil
    @Environment(\.widgetFamily) private var environmentFamily
    private var family: WidgetFamily { familyOverride ?? environmentFamily }

    var body: some View {
        Group {
            if let snapshot = entry.snapshot {
                let ranges = UsageRangeOption.allCases.compactMap { range in
                    snapshot.usage?.ranges?[range.rawValue].map { (range, $0) }
                }
                if ranges.isEmpty {
                    // 旧版主程序（快照版本 1）没有时间范围；新版是用量统计读失败了
                    WidgetMessage(text: snapshot.version >= 2 ? Strings.current.noUsage : Strings.current.outdated)
                } else {
                    let paging = Paging(kind: WidgetKind.usage, family: family, pages: ranges.map { [$0.0.rawValue] })
                    let (range, data) = ranges[paging.current]
                    VStack(alignment: .leading, spacing: 0) {
                        content(data)
                            .frame(maxHeight: .infinity, alignment: .topLeading)
                        WidgetFooter(paging: paging) {
                            HStack(spacing: 6) {
                                Text(range.title).foregroundStyle(.secondary)
                                if family == .systemSmall {
                                    // 小尺寸放不下范围名加时间：只在过时时挂一个橙色时钟
                                    if entry.date.timeIntervalSince(generatedTime(snapshot)) >= staleAfter {
                                        Image(systemName: "clock.badge.exclamationmark").foregroundStyle(.orange)
                                    }
                                } else {
                                    UpdatedTime(snapshot: snapshot, now: entry.date, compact: family == .systemMedium)
                                }
                            }
                        }
                        .padding(.top, 6)
                    }
                }
            } else {
                WidgetMessage(text: Strings.current.off)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func bigCost(_ text: String, size: CGFloat) -> some View {
        Text(text)
            .font(.system(size: size, weight: .bold, design: .rounded).monospacedDigit())
            .lineLimit(1)
            .minimumScaleFactor(0.5)
    }

    private func tokensLine(_ totals: UsageTotals) -> some View {
        Text("\(compactNumber(totals.tokens)) \(Strings.current.tokens) · \(totals.requests.formatted()) \(Strings.current.requests)")
            .font(.system(size: 10).monospacedDigit())
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
    }

    private func metrics(_ totals: UsageTotals, size: CGFloat = 13) -> [Metric] {
        [
            Metric(value: totals.cost, label: Strings.current.cost, size: size),
            Metric(value: compactNumber(totals.tokens), label: Strings.current.tokens, size: size),
            Metric(value: totals.requests.formatted(), label: Strings.current.requests, size: size),
            Metric(value: "\(Int((totals.cacheHitRate * 100).rounded()))%", label: Strings.current.cacheHit, size: size),
        ]
    }

    @ViewBuilder
    private func content(_ data: UsageRange) -> some View {
        switch family {
        case .systemSmall:
            VStack(alignment: .leading, spacing: 2) {
                bigCost(data.totals.cost, size: 28)
                tokensLine(data.totals)
                UsageChart(points: data.series, granularity: data.granularity)
                    .padding(.top, 6)
            }
        case .systemLarge:
            VStack(alignment: .leading, spacing: 10) {
                bigCost(data.totals.cost, size: 34)
                HStack(spacing: 16) {
                    ForEach(Array(metrics(data.totals).dropFirst().enumerated()), id: \.offset) { $0.element }
                }
                UsageChart(points: data.series, granularity: data.granularity, showAxis: true)
                    .frame(height: 104)
                Divider().opacity(0.5)
                appList(data, limit: 4)
            }
        case .systemExtraLarge:
            HStack(alignment: .top, spacing: 28) {
                VStack(alignment: .leading, spacing: 8) {
                    bigCost(data.totals.cost, size: 38)
                    UsageChart(points: data.series, granularity: data.granularity, kind: .area, showAxis: true)
                }
                .frame(maxWidth: .infinity, alignment: .topLeading)
                VStack(alignment: .leading, spacing: 14) {
                    HStack(alignment: .top, spacing: 24) {
                        ForEach(Array(metrics(data.totals, size: 17).dropFirst().enumerated()), id: \.offset) { $0.element }
                    }
                    Divider().opacity(0.5)
                    appList(data, limit: 8)
                }
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
        default:
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    bigCost(data.totals.cost, size: 28)
                    Spacer(minLength: 0)
                    // 三个数字（英文「Cache hits」等较长）：给足宽度、可缩小，别和右边图表的坐标轴挤在一起
                    HStack(spacing: 10) {
                        ForEach(Array(metrics(data.totals, size: 12).dropFirst().enumerated()), id: \.offset) {
                            $0.element.fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .minimumScaleFactor(0.8)
                }
                .frame(width: 168, alignment: .leading)
                UsageChart(points: data.series, granularity: data.granularity, showAxis: true)
            }
        }
    }

    @ViewBuilder
    private func appList(_ data: UsageRange, limit: Int) -> some View {
        if data.byApp.isEmpty {
            Text(Strings.current.noUsage)
                .font(.system(size: 11))
                .foregroundStyle(.tertiary)
        } else {
            let maxCost = data.byApp.map(\.costValue).max() ?? 1
            VStack(spacing: 7) {
                ForEach(data.byApp.prefix(limit)) { app in
                    AppShareRow(app: app, maxCost: maxCost)
                }
            }
        }
    }
}

struct UsageWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: WidgetKind.usage, provider: SnapshotProvider()) { entry in
            UsageWidgetView(entry: entry)
                .widgetGlassContent(.panel)
                .containerBackground(for: .widget) { WidgetGlassBackground(style: .panel) }
        }
        .configurationDisplayName(widgetDisplayName(Strings.current.usageTitle))
        .description(Strings.current.usageGallery)
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .systemExtraLarge])
    }
}
