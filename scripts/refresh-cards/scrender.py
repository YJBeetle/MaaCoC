"""将 SC 的二维网格绘制成按顶点坐标定尺寸的 PNG。"""

from __future__ import annotations

import hashlib
import html
import math
import re
import struct
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw


IDENTITY = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
UV_SCALE = 65536.0
PIXELS_PER_UNIT = 1.0


def compose(parent, child):
    """组合两个 Flash 2D 仿射矩阵 (a,b,c,d,tx,ty)。"""
    a, b, c, d, tx, ty = parent
    e, f, g, h, ux, uy = child
    return (a * e + c * f, b * e + d * f,
            a * g + c * h, b * g + d * h,
            a * ux + c * uy + tx, b * ux + d * uy + ty)


class Renderer:
    def __init__(self, sc, decode_ktx):
        self.sc = sc
        self.decode_ktx = decode_ktx
        resources = sc.tab["resources"]
        self.frame_elements_count, self.frame_elements_at = resources.vec(4, 2)
        self.frame_elements = resources.b
        self.banks = resources.tables(6)
        self.matrix_cache = {}
        self.clip_elements_cache = {}
        self.pages = {}

    def matrix(self, bank_index, matrix_index):
        if matrix_index == 0xFFFF:
            return IDENTITY
        key = (bank_index, matrix_index)
        if key not in self.matrix_cache:
            if bank_index >= len(self.banks):
                raise ValueError(f"矩阵组 {bank_index} 越界")
            bank = self.banks[bank_index]
            count, start = bank.vec(0, 24)
            if matrix_index >= count:
                raise ValueError(f"矩阵 {bank_index}:{matrix_index} 越界")
            self.matrix_cache[key] = struct.unpack_from(
                "<6f", bank.b, start + matrix_index * 24)
        return self.matrix_cache[key]

    def page(self, index):
        if index not in self.pages:
            raw = self.sc.page_ktx(index)
            image = self.decode_ktx(raw) if raw is not None else None
            if image is None or image.size != self.sc.page(index):
                raise ValueError(f"纹理页 {index} 无法解码或尺寸不符")
            # Pillow 会在每次 RGBA affine transform 前预乘整张纹理的 alpha。
            # 缓存 RGBa，避免每个三角形都重新转换一张 4096² 图集。
            self.pages[index] = image.convert("RGBa")
        return self.pages[index]

    def clip_elements(self, clip):
        """返回 MovieClip 首帧的元素。"""
        key = id(clip)
        if key not in self.clip_elements_cache:
            count, frames_at = clip.vec(8, 8)
            if not count:
                self.clip_elements_cache[key] = []
            else:
                offset = clip.scalar(9)
                used = struct.unpack_from("<I", clip.b, frames_at)[0]
                if offset + 3 * used > self.frame_elements_count:
                    raise ValueError("MovieClip 帧元素越界")
                self.clip_elements_cache[key] = [struct.unpack_from(
                    "<3H", self.frame_elements,
                    self.frame_elements_at + 2 * (offset + 3 * i))
                    for i in range(used)]
        return self.clip_elements_cache[key]

    def meshes(self, obj_id, transform=IDENTITY, stack=frozenset()):
        """按 MovieClip 首帧顺序递归取 Shape 网格；循环引用不展开。"""
        if obj_id in stack:
            return
        stack = stack | {obj_id}
        for shape in self.sc.shapes_by_id.get(obj_id, ()):
            for _flags, page, count, start in self.sc.commands(shape):
                if count < 3 or page < 0 or page >= len(self.sc.tsets):
                    raise ValueError(f"对象 {obj_id} 的绘制命令无效")
                w, h = self.sc.page(page)
                a, b, c, d, tx, ty = transform
                points = []
                for x, y, u, v in self.sc.vertices(start, count):
                    points.append((a * x + c * y + tx, b * x + d * y + ty,
                                   u / UV_SCALE * w, v / UV_SCALE * h))
                yield page, points
        for clip in self.sc.clips_by_id.get(obj_id, ()):
            children = [struct.unpack_from("<H", clip.b, p)[0]
                        for p in clip.struct_pos(5, 2)]
            bank = clip.scalar(10)
            for child_index, matrix_index, _color in self.clip_elements(clip):
                if child_index >= len(children):
                    raise ValueError(f"MovieClip {obj_id} 的子对象下标越界")
                child_transform = compose(transform, self.matrix(bank, matrix_index))
                yield from self.meshes(children[child_index], child_transform, stack)

    def render(self, obj_id, max_size=1000):
        """首帧按 xy 画布渲染；超出 max_size 时等比缩小。"""
        meshes = list(self.meshes(obj_id))
        if not meshes:
            return None
        xs = [point[0] for _, points in meshes for point in points]
        ys = [point[1] for _, points in meshes for point in points]
        if not all(math.isfinite(v) for v in xs + ys):
            raise ValueError(f"对象 {obj_id} 的顶点坐标无效")
        min_x, min_y = min(xs), min(ys)
        span_x, span_y = max(xs) - min_x, max(ys) - min_y
        if span_x <= 0 or span_y <= 0:
            return None
        scale = PIXELS_PER_UNIT
        natural = (max(1, math.ceil(span_x * scale)),
                   max(1, math.ceil(span_y * scale)))
        effective_scale = min(scale, max_size / max(span_x, span_y))
        size = (min(max_size, max(1, math.ceil(span_x * effective_scale))),
                min(max_size, max(1, math.ceil(span_y * effective_scale))))
        canvas = Image.new("RGBA", size)
        for page, points in meshes:
            texture = self.page(page)
            for i in range(len(points) - 2):
                triangle = points[i:i + 3]
                dest = [((p[0] - min_x) * effective_scale,
                         (p[1] - min_y) * effective_scale) for p in triangle]
                source = [(p[2], p[3]) for p in triangle]
                (x0, y0), (x1, y1), (x2, y2) = dest
                dx1, dy1, dx2, dy2 = x1 - x0, y1 - y0, x2 - x0, y2 - y0
                determinant = dx1 * dy2 - dx2 * dy1
                if abs(determinant) < 1e-8:
                    continue
                (u0, v0), (u1, v1), (u2, v2) = source
                ua = ((u1 - u0) * dy2 - (u2 - u0) * dy1) / determinant
                ub = (dx1 * (u2 - u0) - dx2 * (u1 - u0)) / determinant
                va = ((v1 - v0) * dy2 - (v2 - v0) * dy1) / determinant
                vb = (dx1 * (v2 - v0) - dx2 * (v1 - v0)) / determinant
                uc = u0 - ua * x0 - ub * y0
                vc = v0 - va * x0 - vb * y0
                left = max(0, math.floor(min(p[0] for p in dest)))
                top = max(0, math.floor(min(p[1] for p in dest)))
                right = min(size[0], math.ceil(max(p[0] for p in dest)))
                bottom = min(size[1], math.ceil(max(p[1] for p in dest)))
                if right <= left or bottom <= top:
                    continue
                region_size = (right - left, bottom - top)
                tile = texture.transform(region_size, Image.Transform.AFFINE,
                                         (ua, ub, ua * left + ub * top + uc,
                                          va, vb, va * left + vb * top + vc),
                                         resample=Image.Resampling.BILINEAR).convert("RGBA")
                mask = Image.new("L", region_size)
                ImageDraw.Draw(mask).polygon(
                    [(x - left, y - top) for x, y in dest], fill=255)
                tile.putalpha(ImageChops.multiply(tile.getchannel("A"), mask))
                canvas.alpha_composite(tile, (left, top))
        return canvas, natural, sorted({page for page, _ in meshes})


def export(sc, out: Path, decode_ktx, max_size=1000):
    """导出每个符号的首帧 PNG 和浏览页。"""
    renderer = Renderer(sc, decode_ktx)
    out.mkdir(parents=True, exist_ok=True)
    lines = ["<!doctype html>", '<meta charset="utf-8">',
             "<title>SC 符号渲染预览</title>",
             "<style>body{font:14px system-ui;background:#1b1b1b;color:#eee;margin:24px}"
             ".grid{display:flex;flex-wrap:wrap;gap:16px}figure{margin:0;width:270px}"
             "img{max-width:256px;max-height:256px;background:#444}"
             "figcaption{overflow-wrap:anywhere;font-size:12px;color:#aaa}</style>",
             "<h1>SC 符号首帧渲染</h1>", '<div class="grid">']
    exported, skipped, failed = 0, [], []
    symbols = sorted(dict(sc.exports()).items())
    total = len(symbols)
    for index, (name, obj_id) in enumerate(symbols, 1):
        if index > 1 and (index - 1) % 100 == 0:
            done = index - 1
            print(f"渲染进度：{done:,}/{total:,}（{done / total:.0%}）", flush=True)
        try:
            result = renderer.render(obj_id, max_size)
        except ValueError as exc:
            failed.append((name, str(exc)))
            continue
        if result is None:
            skipped.append(name)
            continue
        image, natural, pages = result
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("._")[:80] or "symbol"
        suffix = hashlib.sha256(name.encode("utf-8")).hexdigest()[:10]
        filename = f"{safe}--{suffix}"
        if image.size != natural:
            filename += f"--{natural[0]}x{natural[1]}-to-{image.width}x{image.height}"
        filename += ".png"
        image.save(out / filename)
        lines.append(f'<figure><img loading="lazy" src="{html.escape(filename, quote=True)}" '
                     f'alt="{html.escape(name, quote=True)}">'
                     f'<figcaption>{html.escape(name)} · 顶点尺寸 {natural[0]}×{natural[1]} '
                     f'· PNG {image.width}×{image.height} · 页 {pages}</figcaption></figure>')
        exported += 1
    if total:
        print(f"渲染进度：{total:,}/{total:,}（100%）", flush=True)
    lines.append("</div>")
    if failed:
        lines.append("<h2>未能渲染</h2><ul>")
        for name, reason in failed:
            lines.append(f"<li>{html.escape(name)}：{html.escape(reason)}</li>")
        lines.append("</ul>")
    (out / "index.html").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return exported, skipped, failed
