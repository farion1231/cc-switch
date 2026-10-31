// 不装进系统也能看排版：把各小组件、各尺寸按示例数据渲染成 PNG（没有玻璃，背景是平铺的灰）。
// 用法：macos-widget/preview.sh [snapshot.json] → src-tauri/target/macos-widget/preview-*.png

import AppKit
import SwiftUI
import WidgetKit

let args = CommandLine.arguments
let outDir = URL(fileURLWithPath: args[1])
var base = WidgetSnapshot.sample
if args.count > 2 {
    base = try JSONDecoder().decode(WidgetSnapshot.self, from: Data(contentsOf: URL(fileURLWithPath: args[2])))
}
// 给了真实快照就原样用；示例数据换成 16 个账号的那份
let many = args.count > 2 ? SnapshotEntry(date: .now, snapshot: base) : SnapshotEntry(date: .now, snapshot: base.with(accounts: WidgetSnapshot.sampleAccountsMany))
let entry = SnapshotEntry(date: .now, snapshot: base)

func size(_ family: WidgetFamily) -> CGSize {
    switch family {
    case .systemSmall: return CGSize(width: 170, height: 170)
    case .systemLarge: return CGSize(width: 364, height: 382)
    case .systemExtraLarge: return CGSize(width: 752, height: 382)
    default: return CGSize(width: 364, height: 170)
    }
}

@MainActor
func render(_ name: String, _ family: WidgetFamily, _ scheme: ColorScheme, @ViewBuilder _ content: () -> some View) throws {
    let s = size(family)
    let view = content()
        .padding(16)
        .frame(width: s.width, height: s.height)
        .background(scheme == .dark ? Color(white: 0.17) : Color(white: 0.94))
        .containerShape(.rect(cornerRadius: 22))
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .environment(\.colorScheme, scheme)
    let renderer = ImageRenderer(content: view)
    renderer.scale = 2
    guard let image = renderer.nsImage, let tiff = image.tiffRepresentation,
          let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
    else { throw NSError(domain: "preview", code: 1) }
    let url = outDir.appendingPathComponent("preview-\(name)-\(scheme == .dark ? "dark" : "light").png")
    try png.write(to: url)
    print(url.path)
}

try MainActor.assumeIsolated {
    for scheme in [ColorScheme.light, .dark] {
        try render("quota-medium", .systemMedium, scheme) { QuotaWidgetView(entry: entry) }
        try render("quota-large", .systemLarge, scheme) { QuotaWidgetView(entry: entry, familyOverride: .systemLarge) }
        try render("accounts-medium", .systemMedium, scheme) { AccountsWidgetView(entry: many, familyOverride: .systemMedium) }
        try render("accounts-large", .systemLarge, scheme) { AccountsWidgetView(entry: many, familyOverride: .systemLarge) }
        try render("accounts-xl", .systemExtraLarge, scheme) { AccountsWidgetView(entry: many, familyOverride: .systemExtraLarge) }
        try render("usage-small", .systemSmall, scheme) { UsageWidgetView(entry: entry, familyOverride: .systemSmall) }
        try render("usage-medium", .systemMedium, scheme) { UsageWidgetView(entry: entry, familyOverride: .systemMedium) }
        try render("usage-large", .systemLarge, scheme) { UsageWidgetView(entry: entry, familyOverride: .systemLarge) }
    }
    // 用量的四种样式 × 三个尺寸（环境变量 CCSW_RANGE 选范围，默认近 7 天）
    let range = UsageRangeOption(rawValue: ProcessInfo.processInfo.environment["CCSW_RANGE"] ?? "") ?? .last7d
    for style in [UsageStyleOption.overview, .trend, .apps, .metrics] {
        for (name, family) in [("s", WidgetFamily.systemSmall), ("m", .systemMedium), ("l", .systemLarge)] {
            try render("usage-\(style.rawValue)-\(name)", family, .light) {
                UsageWidgetView(entry: entry, range: range, style: style, familyOverride: family)
            }
        }
    }
}
