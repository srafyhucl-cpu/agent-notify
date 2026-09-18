#!/usr/bin/env python3
"""从设计稿生成 Agent-notify 的图标产物。

设计稿由大模型生成，作为图标唯一来源；本脚本只做「换底色 + 裁透明角 + 缩放 + 打包」，
不做任何矢量重绘。详见 docs/superpowers/specs/2026-09-18-app-icon-design.md。

产物：
    assets/agent-notify.ico                          多尺寸应用图标
    internal/ui/assets/tray_{ready,warning,stopped}.png  托盘三态位图（16×16）

用法：
    python tools/build-icon.py
    python tools/build-icon.py --preview C:\\Temp\\icon-preview.png
"""

from __future__ import annotations

import argparse
import os

from PIL import Image, ImageDraw

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SOURCE = os.path.join(REPO_ROOT, "assets", "icon-source.png")
DEFAULT_OUTPUT = os.path.join(REPO_ROOT, "assets", "agent-notify.ico")
DEFAULT_TRAY_DIR = os.path.join(REPO_ROOT, "internal", "ui", "assets")

# 输出尺寸（像素）。顺序即 ICO 目录里的条目顺序。
ICON_SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
# 设计稿里圆角方块的圆角半径（相对画布边长），用于把四角裁成透明。
TILE_RADIUS = 0.166
# 生成透明遮罩时的超采样倍率，保证圆角边缘平滑。
MASK_SUPERSAMPLE = 4

# 托盘图标尺寸（像素），与 SM_CXSMICON 在 100% DPI 下的取值一致。
TRAY_SIZE = 16
# 设计稿底板的「绿度」：g 通道比 r/b 中较小者高出的数值。用于区分彩色底板与白色气泡，
# 取值来自设计稿实测（顶部 136、底部 132）。
BACKGROUND_CHROMA = 134
# 托盘三态底板的垂直渐变，明度关系对齐设计稿；色相取内部状态色常量。
TRAY_BACKGROUNDS = {
    "ready": ((0x55, 0xDD, 0x9B), (0x18, 0x9C, 0x84)),
    "warning": ((0xDD, 0xA9, 0x55), (0x9A, 0x68, 0x18)),
    "stopped": ((0xDD, 0x55, 0x55), (0x9A, 0x18, 0x18)),
}


def tile_mask(size: int) -> Image.Image:
    """返回 size×size 的圆角方块遮罩（L 通道）。"""
    big = size * MASK_SUPERSAMPLE
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, big - 1, big - 1], radius=TILE_RADIUS * big, fill=255
    )
    return mask.resize((size, size), Image.LANCZOS)


def vertical_gradient(size: int, top: tuple, bottom: tuple) -> Image.Image:
    """返回 size×size 的垂直渐变 RGB 图。"""
    strip = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1) if size > 1 else 0.0
        strip.putpixel((0, y), tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)))
    return strip.resize((size, size), Image.NEAREST)


def white_coverage(source: Image.Image) -> Image.Image:
    """估出设计稿里白色图形（气泡 + 星星）的覆盖率，返回 L 通道。"""
    coverage = Image.new("L", source.size)
    src = source.load()
    dst = coverage.load()
    for y in range(source.height):
        for x in range(source.width):
            r, g, b, _ = src[x, y]
            chroma = g - min(r, b)
            dst[x, y] = max(0, min(255, round((BACKGROUND_CHROMA - chroma) / BACKGROUND_CHROMA * 255)))
    return coverage


def recolor_tile(source: Image.Image, top: tuple, bottom: tuple) -> Image.Image:
    """把设计稿底板换成指定渐变，白色图形原样保留（中心星镂空随之显示新底色）。"""
    gradient = vertical_gradient(source.width, top, bottom)
    white = Image.new("RGB", source.size, (255, 255, 255))
    return Image.composite(white, gradient, white_coverage(source))


def build_tray_icons(source_path: str, output_dir: str) -> None:
    source = Image.open(source_path).convert("RGBA")
    os.makedirs(output_dir, exist_ok=True)
    for name, (top, bottom) in TRAY_BACKGROUNDS.items():
        tile = recolor_tile(source, top, bottom)
        frame = tile.resize((TRAY_SIZE, TRAY_SIZE), Image.LANCZOS)
        frame.putalpha(tile_mask(TRAY_SIZE))
        path = os.path.join(output_dir, "tray_%s.png" % name)
        frame.save(path)
        print("wrote %s" % path)


def render(source: Image.Image, size: int) -> Image.Image:
    """把设计稿缩到目标尺寸，并把圆角以外的区域设为透明。"""
    frame = source.resize((size, size), Image.LANCZOS)
    frame.putalpha(tile_mask(size))
    return frame


def build_ico(source_path: str, output: str) -> None:
    source = Image.open(source_path).convert("RGBA")
    if source.width != source.height:
        raise ValueError("design source must be square, got %dx%d" % source.size)
    frames = [render(source, size) for size in ICON_SIZES]
    os.makedirs(os.path.dirname(output), exist_ok=True)
    # bitmap_format="bmp"：ICO 内各条目用 BMP(DIB) 而非 PNG，兼容性最好。
    # append_images：每档尺寸用自己缩放好的帧，避免 Pillow 再从 256 统一缩放一次。
    frames[-1].save(
        output,
        format="ICO",
        sizes=[(size, size) for size in ICON_SIZES],
        append_images=frames[:-1],
        bitmap_format="bmp",
    )
    print("wrote %s (%d sizes)" % (output, len(ICON_SIZES)))


def build_preview(source_path: str, output: str) -> None:
    """拼一张预览图：设计稿 256 | 256 成品 | 小尺寸放大 | 托盘三态放大。"""
    source = Image.open(source_path).convert("RGBA")
    cell = 256
    pad = 16
    small = [size for size in ICON_SIZES if size <= 48]

    sheet = Image.new("RGB", (cell * 2 + pad * 3, cell + pad * 2), (245, 245, 245))
    sheet.paste(source.resize((cell, cell), Image.LANCZOS).convert("RGB"), (pad, pad))
    art = Image.new("RGB", (cell, cell), (245, 245, 245))
    art.paste(render(source, cell), (0, 0), render(source, cell))
    sheet.paste(art, (pad * 2 + cell, pad))

    strip_h = 96
    strip = Image.new("RGB", (len(small) * (strip_h + pad) + pad, strip_h + pad * 2), (245, 245, 245))
    x = pad
    for size in small:
        frame = render(source, size).resize((strip_h, strip_h), Image.NEAREST)
        strip.paste(frame, (x, pad), frame)
        x += strip_h + pad

    tray_h = 128
    tray = Image.new("RGB", (len(TRAY_BACKGROUNDS) * (tray_h + pad) + pad, tray_h + pad * 2), (245, 245, 245))
    x = pad
    for name, (top, bottom) in TRAY_BACKGROUNDS.items():
        frame = recolor_tile(source, top, bottom).resize((TRAY_SIZE, TRAY_SIZE), Image.LANCZOS)
        frame.putalpha(tile_mask(TRAY_SIZE))
        frame = frame.resize((tray_h, tray_h), Image.NEAREST)
        tray.paste(frame, (x, pad), frame)
        x += tray_h + pad

    combined = Image.new("RGB", (max(sheet.width, strip.width, tray.width), sheet.height + strip.height + tray.height), (245, 245, 245))
    combined.paste(sheet, (0, 0))
    combined.paste(strip, (0, sheet.height))
    combined.paste(tray, (0, sheet.height + strip.height))
    combined.save(output)
    print("wrote %s" % output)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build the Agent-notify icon assets from the design source.")
    parser.add_argument("--source", default=DEFAULT_SOURCE, help="square design source PNG")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="ICO output path")
    parser.add_argument("--tray-dir", default=DEFAULT_TRAY_DIR, help="tray PNG output directory")
    parser.add_argument("--preview", help="also write a preview sheet PNG to this path")
    args = parser.parse_args()

    build_ico(args.source, args.output)
    build_tray_icons(args.source, args.tray_dir)
    if args.preview:
        build_preview(args.source, args.preview)


if __name__ == "__main__":
    main()
