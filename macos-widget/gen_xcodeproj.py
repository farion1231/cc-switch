#!/usr/bin/env python3
"""按 Sources/ 和 Resources/ 生成 CCSwitchWidget.xcodeproj（build.sh 每次构建前调用）。

小组件扩展要用 Xcode 构建：app 扩展的 `_NSExtensionMain` 入口、可配置小组件要的
Metadata.appintents（「编辑小组件」里的选项）都只有 Xcode 的构建流程会生成。
工程只是 Sources/ 的一层壳，不进版本库；增删 Swift 文件或语言后重新生成即可。
用法：gen_xcodeproj.py <输出目录>
"""
import hashlib
import os
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
TARGET = "CCSwitchWidget"


def oid(*parts):
    return hashlib.md5("/".join(parts).encode()).hexdigest()[:24].upper()


def q(value):
    # pbxproj 里的字符串：只含安全字符时不加引号
    if value and all((c.isascii() and c.isalnum()) or c in "._/$()-" for c in value):
        return value
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main(out_dir):
    sources = sorted(f for f in os.listdir(os.path.join(ROOT, "Sources")) if f.endswith(".swift"))
    res = os.path.join(ROOT, "Resources")
    langs = sorted(d[: -len(".lproj")] for d in os.listdir(res) if d.endswith(".lproj"))
    # 每个 .strings 文件名一个变体组（Localizable、AppStrings…），有哪些语言就挂哪些
    tables = sorted({f for lang in langs for f in os.listdir(os.path.join(res, f"{lang}.lproj")) if f.endswith(".strings")})
    proj_dir = os.path.relpath(ROOT, out_dir)

    o = {}  # id -> 文本
    main_group, sources_group, resources_group, products_group = (oid("group", n) for n in ("main", "Sources", "Resources", "Products"))
    product = oid("product")
    target = oid("target")
    project = oid("project")
    phase_src, phase_fw, phase_res = oid("phase", "sources"), oid("phase", "frameworks"), oid("phase", "resources")
    info = oid("file", "Info.plist")

    src_files, src_builds = [], []
    for name in sources:
        f, b = oid("file", name), oid("build", name)
        o[f] = f"{{isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = {q(name)}; sourceTree = \"<group>\"; }}"
        o[b] = f"{{isa = PBXBuildFile; fileRef = {f}; }}"
        src_files.append(f)
        src_builds.append(b)
    table_groups, table_builds = [], []
    for table in tables:
        group, build = oid("variant", table), oid("build", table)
        children = []
        for lang in langs:
            if not os.path.isfile(os.path.join(res, f"{lang}.lproj", table)):
                continue
            f = oid("file", lang, table)
            o[f] = f"{{isa = PBXFileReference; lastKnownFileType = text.plist.strings; name = {q(lang)}; path = {q(lang + '.lproj/' + table)}; sourceTree = \"<group>\"; }}"
            children.append(f)
        o[group] = f"{{isa = PBXVariantGroup; children = ({', '.join(children)}, ); name = {q(table)}; sourceTree = \"<group>\"; }}"
        o[build] = f"{{isa = PBXBuildFile; fileRef = {group}; }}"
        table_groups.append(group)
        table_builds.append(build)
    o[info] = "{isa = PBXFileReference; lastKnownFileType = text.plist.xml; path = Info.plist; sourceTree = \"<group>\"; }"
    o[product] = f"{{isa = PBXFileReference; explicitFileType = \"wrapper.app-extension\"; includeInIndex = 0; path = {TARGET}.appex; sourceTree = BUILT_PRODUCTS_DIR; }}"

    o[sources_group] = f"{{isa = PBXGroup; children = ({', '.join(src_files)}, ); path = Sources; sourceTree = \"<group>\"; }}"
    # 图标资源目录（sync_icons.py 生成），xcodebuild 编成 Assets.car
    assets, assets_build = oid("file", "Assets.xcassets"), oid("build", "Assets.xcassets")
    has_assets = os.path.isdir(os.path.join(res, "Assets.xcassets"))
    if has_assets:
        o[assets] = "{isa = PBXFileReference; lastKnownFileType = folder.assetcatalog; path = Assets.xcassets; sourceTree = \"<group>\"; }"
        o[assets_build] = f"{{isa = PBXBuildFile; fileRef = {assets}; }}"
        table_groups.append(assets)
        table_builds.append(assets_build)
    o[resources_group] = f"{{isa = PBXGroup; children = ({', '.join(table_groups)}, ); path = Resources; sourceTree = \"<group>\"; }}"
    o[products_group] = f"{{isa = PBXGroup; children = ({product}, ); name = Products; sourceTree = \"<group>\"; }}"
    o[main_group] = f"{{isa = PBXGroup; children = ({sources_group}, {resources_group}, {info}, {products_group}, ); path = {q(proj_dir)}; sourceTree = \"<group>\"; }}"

    o[phase_src] = f"{{isa = PBXSourcesBuildPhase; buildActionMask = 2147483647; files = ({', '.join(src_builds)}, ); runOnlyForDeploymentPostprocessing = 0; }}"
    o[phase_fw] = "{isa = PBXFrameworksBuildPhase; buildActionMask = 2147483647; files = ( ); runOnlyForDeploymentPostprocessing = 0; }"
    o[phase_res] = f"{{isa = PBXResourcesBuildPhase; buildActionMask = 2147483647; files = ({', '.join(table_builds)}, ); runOnlyForDeploymentPostprocessing = 0; }}"

    target_settings = {
        "PRODUCT_NAME": TARGET,
        "PRODUCT_BUNDLE_IDENTIFIER": "com.ccswitch.desktop.widget",
        # 相对工程所在目录（SRCROOT）：工程生成在 target 目录里
        "INFOPLIST_FILE": os.path.join(proj_dir, "Info.plist"),
        "GENERATE_INFOPLIST_FILE": "NO",
        "MACOSX_DEPLOYMENT_TARGET": "14.0",
        "SWIFT_VERSION": "5.0",
        "APPLICATION_EXTENSION_API_ONLY": "YES",
        "SKIP_INSTALL": "YES",
        "LD_RUNPATH_SEARCH_PATHS": "@executable_path/../Frameworks @executable_path/../../../../Frameworks",
        # 签名在 build.sh 里按自己的 entitlements 做
        "CODE_SIGNING_ALLOWED": "NO",
        "CODE_SIGN_IDENTITY": "",
        "ONLY_ACTIVE_ARCH": "NO",
        "SWIFT_EMIT_LOC_STRINGS": "YES",
    }
    project_settings = {"SDKROOT": "macosx", "ALWAYS_SEARCH_USER_PATHS": "NO"}

    def config_list(kind, settings):
        ids = []
        for name in ("Debug", "Release"):
            cid = oid("config", kind, name)
            extra = dict(settings)
            if kind == "project":
                extra["SWIFT_OPTIMIZATION_LEVEL"] = "-Onone" if name == "Debug" else "-O"
                if name == "Debug":
                    extra["SWIFT_ACTIVE_COMPILATION_CONDITIONS"] = "DEBUG"
            body = " ".join(f"{k} = {q(v)};" for k, v in extra.items())
            o[cid] = f"{{isa = XCBuildConfiguration; buildSettings = {{ {body} }}; name = {name}; }}"
            ids.append(cid)
        lid = oid("configlist", kind)
        o[lid] = f"{{isa = XCConfigurationList; buildConfigurations = ({', '.join(ids)}, ); defaultConfigurationIsVisible = 0; defaultConfigurationName = Release; }}"
        return lid

    o[target] = (
        f"{{isa = PBXNativeTarget; buildConfigurationList = {config_list('target', target_settings)}; "
        f"buildPhases = ({phase_src}, {phase_fw}, {phase_res}, ); buildRules = ( ); dependencies = ( ); "
        f"name = {TARGET}; productName = {TARGET}; productReference = {product}; "
        f"productType = \"com.apple.product-type.app-extension\"; }}"
    )
    regions = ", ".join(q(r) for r in ["en", "Base", *[l for l in langs if l != "en"]])
    o[project] = (
        f"{{isa = PBXProject; attributes = {{ BuildIndependentTargetsInParallel = 1; LastSwiftUpdateCheck = 1600; LastUpgradeCheck = 1600; }}; "
        f"buildConfigurationList = {config_list('project', project_settings)}; compatibilityVersion = \"Xcode 14.0\"; "
        f"developmentRegion = en; hasScannedForEncodings = 0; knownRegions = ({regions}, ); mainGroup = {main_group}; "
        f"productRefGroup = {products_group}; projectDirPath = \"\"; projectRoot = \"\"; targets = ({target}, ); }}"
    )

    lines = ["// !$*UTF8*$!", "{", "\tarchiveVersion = 1;", "\tclasses = {", "\t};", "\tobjectVersion = 56;", "\tobjects = {"]
    for key in sorted(o):
        lines.append(f"\t\t{key} = {o[key]};")
    lines += ["\t};", f"\trootObject = {project};", "}", ""]

    path = os.path.join(out_dir, f"{TARGET}.xcodeproj")
    os.makedirs(path, exist_ok=True)
    with open(os.path.join(path, "project.pbxproj"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))
    print(path)


if __name__ == "__main__":
    main(os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else ROOT))
