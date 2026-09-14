#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""sync_preview.py — 把 sif-studio/dist 同步到 sif-studio-preview/app

要点：
  - 保留 preview 自己的 index.html 模板（含 mock-tauri.js 引用）
  - 只替换 assets 的 hash 引用
  - ⚠️ dist/assets 里可能有 **多个 .js**（vite 动态 import 会切出额外 chunk，
    例如 M2.5 的 @tauri-apps/plugin-dialog chunk 只有 ~1KB）。
    因此绝不能「取最后一个 .js」，必须从 dist/index.html 解析真实入口。
    （2026-09-13 血泪：按 listdir 顺序取到 dialog chunk → 页面白屏）
  - 动态 chunk 也要一起拷贝（运行时按需 fetch），所以 assets 整目录复制
"""
import os
import re
import shutil
import sys

ROOT = r"C:\Users\Beta\WorkBuddy\2026-09-12-09-46-53"
SRC = os.path.join(ROOT, "sif-studio", "dist")
DST = os.path.join(ROOT, "sif-studio-preview", "app")

TEMPLATE = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="X-UA-Compatible" content="IE=edge" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>SIF Studio / 联锁工坊 — 可交互预览</title>
  <link rel="icon" type="image/svg+xml" href="./favicon.svg" />
  <script src="./mock-tauri.js"></script>
  <script type="module" crossorigin src="./assets/{js}"></script>
  <link rel="stylesheet" crossorigin href="./assets/{css}">
</head>
<body>
  <div id="app"></div>
</body>
</html>
"""


def pick_entry(dist_html: str, src_assets: str):
    """从 dist/index.html 解析真实入口 js/css；解析不到则按体积兜底（最大 = 主 chunk）。"""
    m_js = re.search(r'<script[^>]*type="module"[^>]*src="([^"]+)"', dist_html)
    m_css = re.search(r'<link[^>]*rel="stylesheet"[^>]*href="([^"]+)"', dist_html)

    js_name = os.path.basename(m_js.group(1)) if m_js else None
    css_name = os.path.basename(m_css.group(1)) if m_css else None

    files = os.listdir(src_assets)
    if not js_name or not os.path.exists(os.path.join(src_assets, js_name)):
        # 兜底：体积最大的 .js（主 chunk 必然最大；动态 chunk 都很小）
        cand = [f for f in files if f.endswith(".js")]
        if not cand:
            return None, None
        js_name = max(cand, key=lambda f: os.path.getsize(os.path.join(src_assets, f)))
    if not css_name or not os.path.exists(os.path.join(src_assets, css_name)):
        cand = [f for f in files if f.endswith(".css")]
        if not cand:
            return js_name, None
        css_name = max(cand, key=lambda f: os.path.getsize(os.path.join(src_assets, f)))

    return js_name, css_name


def main() -> int:
    if not os.path.isdir(SRC):
        print(f"ERROR: dist not found: {SRC}", file=sys.stderr)
        return 1

    src_assets = os.path.join(SRC, "assets")
    dist_index = os.path.join(SRC, "index.html")
    if not os.path.isdir(src_assets) or not os.path.isfile(dist_index):
        print("ERROR: dist/assets or dist/index.html missing", file=sys.stderr)
        return 1

    with open(dist_index, encoding="utf-8") as fh:
        dist_html = fh.read()

    js_name, css_name = pick_entry(dist_html, src_assets)
    if not js_name or not css_name:
        print("ERROR: cannot resolve entry js/css", file=sys.stderr)
        return 1

    # 打印全部 chunk，便于确认动态 chunk 也被带上
    all_js = sorted(f for f in os.listdir(src_assets) if f.endswith(".js"))
    extra = [f for f in all_js if f != js_name]

    # 1. 替换 DST/assets（整目录，动态 chunk 也要）
    dst_assets = os.path.join(DST, "assets")
    if os.path.exists(dst_assets):
        if os.path.isdir(dst_assets):
            shutil.rmtree(dst_assets)
        else:
            os.remove(dst_assets)
    shutil.copytree(src_assets, dst_assets)

    # 1'. 复制 public/editor.html（M2.8.4 起 editor.html 也改，本脚本只动
    #     assets 的话会让 preview 一直停在旧版上 —— 验证/截图全都白跑）。
    src_editor = os.path.join(SRC, "editor.html")
    dst_editor = os.path.join(DST, "editor.html")
    if os.path.isfile(src_editor):
        shutil.copy2(src_editor, dst_editor)

    # 2. 写 index.html（保留 mock-tauri.js）
    with open(os.path.join(DST, "index.html"), "w", encoding="utf-8") as fh:
        fh.write(TEMPLATE.format(js=js_name, css=css_name))

    print("sync ok")
    print(f"  js      = {js_name}")
    print(f"  css     = {css_name}")
    if extra:
        print(f"  chunks  = {', '.join(extra)}")
    if os.path.isfile(src_editor):
        print(f"  editor  = editor.html ({os.path.getsize(src_editor)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
