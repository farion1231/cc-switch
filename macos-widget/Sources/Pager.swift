// 小组件里翻页（参考 CodexBar 的 provider 切换）：底栏右边 ‹ n/m ›，点一下跑 `WidgetPageIntent`。
//
// - 记的是「这一页从哪一项开始」（应用 / 账号 / 时间范围的 id），不是页码：数据增减、
//   排序变了，也还停在那一项所在的那页；那一项没了就回第一页。
// - 按「小组件种类 + 尺寸」分开记：同一种的大、小两个实例各翻各的，同种同尺寸的共用一页。
// - 按钮的意图在扩展进程里跑，记在扩展自己的 UserDefaults 里，不经过主程序。

import AppIntents
import SwiftUI
import WidgetKit

enum WidgetKind {
    static let quota = "com.ccswitch.desktop.widget.quota"
    static let accounts = "com.ccswitch.desktop.widget.accounts"
    static let usage = "com.ccswitch.desktop.widget.usage"
}

enum PageStore {
    static func key(kind: String, family: WidgetFamily) -> String {
        "widgetPage.\(kind).\(family)"
    }

    static func anchor(_ key: String) -> String? {
        UserDefaults.standard.string(forKey: key)
    }

    static func setAnchor(_ anchor: String, for key: String) {
        UserDefaults.standard.set(anchor, forKey: key)
    }

    /// 记下的那一项在第几页；没记过或那一项已经没了就是第一页。
    static func currentPage(_ pages: [[String]], key: String) -> Int {
        guard let anchor = anchor(key) else { return 0 }
        return pages.firstIndex { $0.contains(anchor) } ?? 0
    }
}

struct WidgetPageIntent: AppIntent {
    static let title = LocalizedStringResource("pager.intent.title", defaultValue: "Turn Page")
    static let isDiscoverable = false

    @Parameter(title: "Page Key") var key: String
    @Parameter(title: "Widget Kind") var kind: String
    @Parameter(title: "First Item") var anchor: String

    init() {}

    init(key: String, kind: String, anchor: String) {
        self.key = key
        self.kind = kind
        self.anchor = anchor
    }

    func perform() async throws -> some IntentResult {
        PageStore.setAnchor(anchor, for: key)
        WidgetCenter.shared.reloadTimelines(ofKind: kind)
        return .result()
    }
}

/// 一组页：每页的 id，和现在在第几页。
struct Paging {
    let kind: String
    let key: String
    /// 每页第一项的 id（翻到那页时记下它）
    let anchors: [String]
    let current: Int

    init(kind: String, family: WidgetFamily, pages: [[String]]) {
        self.kind = kind
        key = PageStore.key(kind: kind, family: family)
        anchors = pages.compactMap(\.first)
        current = min(PageStore.currentPage(pages, key: key), max(0, pages.count - 1))
    }
}

/// 底栏右边的 ‹ n/m ›；只有一页时不画。首尾相接：最后一页再往后回到第一页。
struct PagerControl: View {
    let paging: Paging

    var body: some View {
        let count = paging.anchors.count
        if count > 1 {
            HStack(spacing: 1) {
                button("chevron.left", label: Strings.current.previousPage, to: (paging.current - 1 + count) % count)
                Text("\(paging.current + 1)/\(count)")
                    .monospacedDigit()
                    .fixedSize()
                button("chevron.right", label: Strings.current.nextPage, to: (paging.current + 1) % count)
            }
            .foregroundStyle(.secondary)
        }
    }

    private func button(_ symbol: String, label: String, to page: Int) -> some View {
        Button(intent: WidgetPageIntent(key: paging.key, kind: paging.kind, anchor: paging.anchors[page])) {
            Image(systemName: symbol)
                .font(.system(size: 9, weight: .bold))
                .frame(width: 18, height: 18)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}

/// 三个小组件共用的底栏：左边写附加信息（时间 / 账号数 / 时间范围），右边翻页。
struct WidgetFooter<Leading: View>: View {
    static var height: CGFloat { 18 }

    let paging: Paging?
    @ViewBuilder var leading: Leading

    var body: some View {
        HStack(spacing: 6) {
            leading
                .lineLimit(1)
            Spacer(minLength: 4)
            if let paging {
                PagerControl(paging: paging)
            }
        }
        .font(.system(size: 10).monospacedDigit())
        .foregroundStyle(.tertiary)
        .frame(height: Self.height)
    }
}

/// 按固定个数分页。
func chunked<T>(_ items: [T], size: Int) -> [[T]] {
    guard size > 0 else { return [items] }
    return stride(from: 0, to: items.count, by: size).map { Array(items[$0 ..< min($0 + size, items.count)]) }
}
