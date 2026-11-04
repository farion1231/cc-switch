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

let families: [(String, WidgetFamily)] = [("small", .systemSmall), ("medium", .systemMedium), ("large", .systemLarge), ("xl", .systemExtraLarge)]

try MainActor.assumeIsolated {
    for scheme in [ColorScheme.light, .dark] {
        for (name, family) in families {
            try render("quota-\(name)", family, scheme) { QuotaWidgetView(entry: entry, familyOverride: family) }
            try render("accounts-\(name)", family, scheme) { AccountsWidgetView(entry: many, familyOverride: family) }
            try render("usage-\(name)", family, scheme) { UsageWidgetView(entry: entry, familyOverride: family) }
        }
    }
}
