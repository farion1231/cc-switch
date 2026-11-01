#!/usr/bin/env python3
"""把小组件要用的供应商 / 应用图标从 app 的图标库（src/icons/extracted）同步成
Resources/Assets.xcassets（build.sh 每次构建都会跑，图标和 app 里一模一样）。

- 单色图标（根节点 fill="currentColor"）按模板图标处理，跟着文字颜色走，深浅色都看得清
- 彩色图标原样保留
- SVG 的 1em 宽高换成 viewBox 的尺寸，Xcode 的 SVG 解析不认 em
- 路径里圆弧（a/A）的两个标志位常写成紧凑形式（`0 01-4.45`），浏览器认，macOS 的 CoreSVG
  会读错（Gemini 只画出一个小三角），这里拆开写
"""
import json
import os
import re
import shutil

ROOT = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(ROOT, "..", "src", "icons", "extracted")
OUT = os.path.join(ROOT, "Resources", "Assets.xcassets")

# 授权中心的渠道 + 用量 / 额度里出现的应用（键见 AuthCenterText.swift 的 brandIcon）
ICONS = [
    "claude", "openai", "githubcopilot", "xai", "kimi", "antigravity", "gemini",
    "grok", "claw", "minimax", "deepseek", "opencode-logo-light", "hermes",
]


NUMBER = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")


def expand_arc_flags(d):
    """把路径数据里圆弧命令的参数逐个读出来，用空格分开（标志位只占一个字符）。"""
    out, i, cmd = [], 0, None
    while i < len(d):
        c = d[i]
        if c.isalpha():
            cmd = c
            out.append(c)
            i += 1
            continue
        if c in " ,\t\n\r":
            i += 1
            continue
        if cmd in ("a", "A"):
            params = []
            for index in range(7):
                while i < len(d) and d[i] in " ,\t\n\r":
                    i += 1
                if index in (3, 4):
                    params.append(d[i])
                    i += 1
                else:
                    m = NUMBER.match(d, i)
                    params.append(m.group(0))
                    i = m.end()
            out.append(" ".join(params))
            continue
        m = NUMBER.match(d, i)
        if not m:
            out.append(c)
            i += 1
            continue
        out.append(m.group(0))
        i = m.end()
    # 命令和数字之间、数字之间都用空格隔开
    return " ".join(out)


def normalize_svg(text):
    text = re.sub(
        r'(<path\b[^>]*?\sd=)(["\'])(.*?)\2',
        lambda m: m.group(1) + m.group(2) + expand_arc_flags(m.group(3)) + m.group(2)
        if re.search(r"[aA]", m.group(3)) else m.group(0),
        text,
        flags=re.S,
    )
    root = re.search(r"<svg\b[^>]*>", text).group(0)
    # 属性值单双引号都有（opencode 的图标用单引号）；viewBox 也可能用逗号分隔
    view_box = re.search(r"""viewBox=(["'])(.*?)\1""", root)
    new_root = re.sub(r"""\s(width|height)=(["']).*?\2""", "", root)
    new_root = re.sub(r"""\sstyle=(["']).*?\1""", "", new_root)
    if view_box:
        _, _, w, h = re.split(r"[\s,]+", view_box.group(2).strip())
        new_root = new_root.replace("<svg", f'<svg width="{w}" height="{h}"', 1)
    return text.replace(root, new_root, 1), re.search(r"""fill=(["'])currentColor\1""", root) is not None


def main():
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)
    with open(os.path.join(OUT, "Contents.json"), "w") as fh:
        json.dump({"info": {"author": "xcode", "version": 1}}, fh, indent=2)
    for name in ICONS:
        svg = os.path.join(SRC, f"{name}.svg")
        png = os.path.join(SRC, f"{name}.png")
        image_set = os.path.join(OUT, f"{name}.imageset")
        os.makedirs(image_set)
        properties = {}
        if os.path.isfile(svg):
            text, template = normalize_svg(open(svg, encoding="utf-8").read())
            filename = f"{name}.svg"
            with open(os.path.join(image_set, filename), "w", encoding="utf-8") as fh:
                fh.write(text)
            properties["preserves-vector-representation"] = True
            properties["template-rendering-intent"] = "template" if template else "original"
        elif os.path.isfile(png):
            filename = f"{name}.png"
            shutil.copy(png, os.path.join(image_set, filename))
            properties["template-rendering-intent"] = "original"
        else:
            raise SystemExit(f"missing icon {name} in src/icons/extracted")
        contents = {
            "images": [{"filename": filename, "idiom": "universal"}],
            "info": {"author": "xcode", "version": 1},
            "properties": properties,
        }
        with open(os.path.join(image_set, "Contents.json"), "w") as fh:
            json.dump(contents, fh, indent=2)


if __name__ == "__main__":
    main()
