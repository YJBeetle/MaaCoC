"""将 SC 的二维网格绘制成按顶点坐标定尺寸的 PNG。"""

from __future__ import annotations

import hashlib
import html
import math
import os
import re
import shutil
import struct
import subprocess
import tempfile
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


def preview_header(title):
    """PNG/WebP 浏览页共用的标题与符号名筛选框。"""
    return ["<!doctype html>", '<meta charset="utf-8">',
            f"<title>{html.escape(title)}</title>",
            "<style>body{font:14px system-ui;background:#1b1b1b;color:#eee;margin:24px}"
            ".filter{margin:0 0 20px;display:flex;align-items:center;gap:12px;flex-wrap:wrap}"
            ".filter input{font:inherit;color:#eee;background:#333;border:1px solid #666;"
            "border-radius:6px;padding:8px 10px;min-width:min(320px,80vw)}"
            ".filter output{color:#aaa}#symbols{display:grid;"
            "grid-template-columns:repeat(auto-fill,minmax(min(100%,300px),1fr));"
            "gap:16px;align-items:start;grid-auto-flow:dense}"
            ".symbol{min-width:0;padding:12px;"
            "border:1px solid #444;border-radius:10px;background:#242424}"
            ".symbol[hidden]{display:none}.symbol h2{font-size:18px;margin:0 0 14px;"
            "overflow-wrap:anywhere}.grid{display:flex;flex-wrap:wrap;gap:16px}"
            "figure{margin:0;width:270px;max-width:100%}"
            ".image-with-text{position:relative;display:inline-block;line-height:0}"
            ".composite{position:relative;display:block;line-height:0;background:#444}"
            ".composite img{position:absolute;inset:0;width:100%;height:100%;"
            "max-width:none;max-height:none;background:transparent}"
            ".overlay-text{position:absolute;left:50%;top:50%;"
            "transform:translate(-50%,-50%);font:18px system-ui;line-height:1;"
            "white-space:nowrap;color:#fff;text-shadow:0 1px 2px #000}"
            ".text-placeholder{width:256px;max-width:100%;height:80px;"
            "display:grid;place-items:center;background:#303030;"
            "color:#eee;font:18px system-ui}"
            ".missing{box-sizing:border-box;width:256px;max-width:100%;height:80px;"
            "display:grid;place-items:center;"
            "background:#303030;color:#aaa;border:1px dashed #666}"
            "img{max-width:256px;max-height:256px;background:#444}"
            "figcaption{overflow-wrap:anywhere;font-size:12px;color:#aaa}</style>",
            f"<h1>{html.escape(title)}</h1>",
            '<div class="filter"><label for="symbol-filter">符号名筛选</label>'
            '<input id="symbol-filter" type="search" placeholder="输入符号名" autocomplete="off">'
            '<output id="match-count" aria-live="polite"></output></div>',
            '<main id="symbols">']


def write_preview(out, lines):
    """收尾并写入支持即时筛选的浏览页。"""
    lines.extend(['<script>',
                  'const input = document.getElementById("symbol-filter");',
                  'const grid = document.getElementById("symbols");',
                  'const groups = [...document.querySelectorAll(".symbol")];',
                  'const count = document.getElementById("match-count");',
                  'function sizeGroups() {',
                  '  const columns = getComputedStyle(grid).gridTemplateColumns.split(" ").length;',
                  '  for (const group of groups) {',
                  '    group.style.gridColumnEnd = `span ${Math.min(+group.dataset.figures, columns)}`;',
                  '  }',
                  '}',
                  'function filterSymbols() {',
                  '  const query = input.value.trim().toLocaleLowerCase();',
                  '  let matches = 0;',
                  '  for (const group of groups) {',
                  '    const visible = group.dataset.name.toLocaleLowerCase().includes(query);',
                  '    group.hidden = !visible;',
                  '    if (visible) matches++;',
                  '  }',
                  '  count.textContent = `${matches} / ${groups.length} 个符号`;',
                  '}',
                  'window.addEventListener("resize", sizeGroups);',
                  'input.addEventListener("input", filterSymbols);',
                  'sizeGroups();',
                  'filterSymbols();',
                  '</script>'])
    (out / "index.html").write_text("\n".join(lines) + "\n", encoding="utf-8")


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
        self.clip_frame_cache = {}
        self.clip_appearance_cache = {}
        self.duration_cache = {}
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
            # SC 图集已经是预乘 alpha。按 RGBa 解释原始字节，避免
            # convert("RGBa") 再乘一次，导致 PNG 和 WebP 的半透明边缘发黑。
            self.pages[index] = Image.frombytes("RGBa", image.size, image.tobytes())
        return self.pages[index]

    def clip_elements(self, clip, frame=0):
        """返回 MovieClip 指定帧的元素。"""
        key = id(clip)
        if key not in self.clip_elements_cache:
            count, frames_at = clip.vec(8, 8)
            item_size, fmt = 8, "<I"
            if not count:
                count, frames_at = clip.vec(12, 2)
                item_size, fmt = 2, "<H"
            offset = clip.scalar(9)
            frames = []
            for i in range(count):
                used = struct.unpack_from(fmt, clip.b, frames_at + item_size * i)[0]
                if offset + 3 * used > self.frame_elements_count:
                    raise ValueError("MovieClip 帧元素越界")
                frames.append((offset, used))
                offset += 3 * used
            self.clip_elements_cache[key] = frames
        frames = self.clip_elements_cache[key]
        if not frames:
            return []
        selected = (key, min(frame, len(frames) - 1))
        if selected not in self.clip_frame_cache:
            offset, used = frames[selected[1]]
            self.clip_frame_cache[selected] = [struct.unpack_from(
                "<3H", self.frame_elements,
                self.frame_elements_at + 2 * (offset + 3 * j))
                for j in range(used)]
        return self.clip_frame_cache[selected]

    def clip_appearances(self, clip):
        """每个直接子对象首次和最后一次出现在父时间轴的帧号。"""
        key = id(clip)
        if key not in self.clip_appearance_cache:
            positions = {}
            for frame in range(animation_frames(clip)):
                for child_index, _matrix, _color in self.clip_elements(clip, frame):
                    if child_index not in positions:
                        positions[child_index] = [frame, frame]
                    else:
                        positions[child_index][1] = frame
            self.clip_appearance_cache[key] = positions
        return self.clip_appearance_cache[key]

    def timeline_frames(self, obj_id, stack=frozenset()):
        """子对象独立播放；父时间轴结束后停在最后一帧。"""
        if obj_id in stack:
            return 1
        if obj_id in self.duration_cache:
            return self.duration_cache[obj_id]
        stack = stack | {obj_id}
        duration = 1
        for clip in self.sc.clips_by_id.get(obj_id, ()):
            parent_count = animation_frames(clip)
            duration = max(duration, parent_count)
            children = [struct.unpack_from("<H", clip.b, p)[0]
                        for p in clip.struct_pos(5, 2)]
            for child_index, (first, last) in self.clip_appearances(clip).items():
                if child_index >= len(children):
                    raise ValueError(f"MovieClip {obj_id} 的子对象下标越界")
                if last == parent_count - 1:
                    duration = max(duration, first + self.timeline_frames(
                        children[child_index], stack))
        self.duration_cache[obj_id] = duration
        return duration

    def solo_meshes(self, obj_id, layer_index, frame):
        """独立图层使用自己的帧；父层结束后固定在最后一次可见的矩阵。"""
        clip = self.sc.clips_by_id[obj_id][0]
        first, last = self.clip_appearances(clip)[layer_index]
        parent_frame = min(first + frame, last)
        for current in range(parent_frame, first - 1, -1):
            elements = [item for item in self.clip_elements(clip, current)
                        if item[0] == layer_index]
            if elements:
                break
        child_at = clip.struct_pos(5, 2)[layer_index]
        child_id = struct.unpack_from("<H", clip.b, child_at)[0]
        bank = clip.scalar(10)
        for _, matrix_index, _color in elements:
            transform = self.matrix(bank, matrix_index)
            yield from self.meshes(child_id, transform, frame=frame)

    def layer_span(self, obj_id, layer_index, child_id):
        """按独立图层的父层变化和自身动画求有效帧数与画布边界。"""
        clip = self.sc.clips_by_id[obj_id][0]
        first, last = self.clip_appearances(clip)[layer_index]
        duration = max(last - first + 1, self.timeline_frames(child_id))
        previous, last_change, bounds = None, None, None
        for frame in range(duration):
            meshes = list(self.solo_meshes(obj_id, layer_index, frame))
            if meshes != previous:
                last_change = frame
            previous = meshes
            if not meshes:
                continue
            current = self.bounds(meshes, obj_id)
            bounds = current if bounds is None else (
                min(bounds[0], current[0]), min(bounds[1], current[1]),
                max(bounds[2], current[2]), max(bounds[3], current[3]))
        if bounds is None:
            return None
        return last_change + 1, bounds

    def meshes(self, obj_id, transform=IDENTITY, stack=frozenset(), frame=0,
               layer=None):
        """按帧取网格；layer 只保留根 MovieClip 的指定子对象。"""
        if obj_id in stack:
            return
        stack = stack | {obj_id}
        for shape in (self.sc.shapes_by_id.get(obj_id, ()) if layer is None else ()):
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
            appearances = self.clip_appearances(clip) if frame else None
            for child_index, matrix_index, _color in self.clip_elements(clip, frame):
                if child_index >= len(children):
                    raise ValueError(f"MovieClip {obj_id} 的子对象下标越界")
                if layer is not None and child_index != layer:
                    continue
                child_transform = compose(transform, self.matrix(bank, matrix_index))
                child_frame = (max(0, frame - appearances[child_index][0])
                               if appearances else 0)
                yield from self.meshes(children[child_index], child_transform,
                                       stack, child_frame)

    def render(self, obj_id, max_size=1000, frame=0, bounds=None, layer=None,
               solo=False):
        """按 xy 画布渲染一帧；bounds 可固定动画各帧画布。"""
        meshes = list(self.solo_meshes(obj_id, layer, frame) if solo else
                      self.meshes(obj_id, frame=frame, layer=layer))
        if not meshes:
            return None
        if bounds is None:
            bounds = self.bounds(meshes, obj_id)
        min_x, min_y, max_x, max_y = bounds
        span_x, span_y = max_x - min_x, max_y - min_y
        if span_x <= 0 or span_y <= 0:
            return None
        scale = PIXELS_PER_UNIT
        natural = (max(1, math.ceil(span_x * scale)),
                   max(1, math.ceil(span_y * scale)))
        effective_scale = min(scale, max_size / max(span_x, span_y))
        size = (min(max_size, max(1, math.ceil(span_x * effective_scale))),
                min(max_size, max(1, math.ceil(span_y * effective_scale))))
        canvas = Image.new("RGBA", size)
        self.draw_meshes(canvas, meshes, bounds,
                         scale=(effective_scale, effective_scale))
        return canvas, natural, sorted({page for page, _ in meshes})

    def draw_meshes(self, canvas, meshes, bounds, scale=None, clip_box=None):
        """把网格直接绘制到已有画布；可限制绘制区域。"""
        min_x, min_y, max_x, max_y = bounds
        if scale is None:
            scale = (canvas.width / (max_x - min_x),
                     canvas.height / (max_y - min_y))
        scale_x, scale_y = scale
        if clip_box is not None:
            clip_left = max(0, math.floor((clip_box[0] - min_x) * scale_x))
            clip_top = max(0, math.floor((clip_box[1] - min_y) * scale_y))
            clip_right = min(canvas.width, math.ceil((clip_box[2] - min_x) * scale_x))
            clip_bottom = min(canvas.height, math.ceil((clip_box[3] - min_y) * scale_y))
        for page, points in meshes:
            texture = self.page(page)
            projected = [((p[0] - min_x) * scale_x,
                          (p[1] - min_y) * scale_y) for p in points]
            mesh_left = max(0, math.floor(min(x for x, _ in projected)))
            mesh_top = max(0, math.floor(min(y for _, y in projected)))
            mesh_right = min(canvas.width, math.ceil(max(x for x, _ in projected)))
            mesh_bottom = min(canvas.height, math.ceil(max(y for _, y in projected)))
            if clip_box is not None:
                mesh_left, mesh_top = max(mesh_left, clip_left), max(mesh_top, clip_top)
                mesh_right, mesh_bottom = min(mesh_right, clip_right), min(mesh_bottom, clip_bottom)
            if mesh_right <= mesh_left or mesh_bottom <= mesh_top:
                continue
            mesh_layer = Image.new("RGBA", (mesh_right - mesh_left,
                                             mesh_bottom - mesh_top))
            for i in range(len(points) - 2):
                triangle = points[i:i + 3]
                dest = projected[i:i + 3]
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
                left = max(mesh_left, math.floor(min(p[0] for p in dest)))
                top = max(mesh_top, math.floor(min(p[1] for p in dest)))
                right = min(mesh_right, math.ceil(max(p[0] for p in dest)))
                bottom = min(mesh_bottom, math.ceil(max(p[1] for p in dest)))
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
                # 同一网格的相邻三角形共用边，边上的像素只能写一次。
                # 逐三角形 alpha_composite 会把半透明像素叠加两遍，形成对角线。
                mesh_layer.paste(tile, (left - mesh_left, top - mesh_top), mask)
            canvas.alpha_composite(mesh_layer, (mesh_left, mesh_top))

    @staticmethod
    def bounds(meshes, obj_id):
        xs = [point[0] for _, points in meshes for point in points]
        ys = [point[1] for _, points in meshes for point in points]
        if not all(math.isfinite(v) for v in xs + ys):
            raise ValueError(f"对象 {obj_id} 的顶点坐标无效")
        return min(xs), min(ys), max(xs), max(ys)


def root_layers(sc, obj_id, visible=None):
    """根 MovieClip 中实际显示的直接子对象；序号保留原始下标。"""
    clips = sc.clips_by_id.get(obj_id, ())
    if not clips:
        return []
    clip = clips[0]
    return [(i, struct.unpack_from("<H", clip.b, p)[0])
            for i, p in enumerate(clip.struct_pos(5, 2))
            if visible is None or i in visible]


def filename_for(name, natural, size, layer=None, extension="png"):
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("._")[:80] or "symbol"
    suffix = hashlib.sha256(name.encode("utf-8")).hexdigest()[:10]
    filename = f"{safe}--{suffix}"
    if size != natural:
        filename += f"--{natural[0]}x{natural[1]}-to-{size[0]}x{size[1]}"
    if layer is not None:
        filename += f"+layer-{layer:02d}"
    return filename + "." + extension


def start_group(lines, name):
    escaped = html.escape(name, quote=True)
    lines.append(f'<section class="symbol" data-name="{escaped}" '
                 'data-figures="0">'
                 f'<h2>{html.escape(name)}</h2><div class="grid">')
    return len(lines) - 1


def end_group(lines, start):
    figures = sum(line.startswith("<figure>") for line in lines[start + 1:])
    lines[start] = lines[start].replace('data-figures="0"',
                                        f'data-figures="{figures}"', 1)
    lines.append("</div></section>")


def add_figure(lines, name, filename, detail, overlay_text=False):
    image = (f'<img loading="lazy" src="{html.escape(filename, quote=True)}" '
             f'alt="{html.escape(name, quote=True)}">')
    if overlay_text:
        image = (f'<div class="image-with-text">{image}'
                 '<span class="overlay-text">######</span></div>')
    lines.append(f'<figure>{image}<figcaption>{html.escape(detail)}'
                 '</figcaption></figure>')


def add_composite_figure(lines, name, filenames, size, overlay_text=False,
                         animated=False):
    """在浏览器中按图层顺序叠图；各动画独立循环。"""
    scale = min(1, 256 / max(size))
    width, height = (max(1, round(value * scale)) for value in size)
    images = "".join(
        f'<img loading="lazy" src="{html.escape(filename, quote=True)}" '
        f'alt="{html.escape(name, quote=True)} 图层">'
        for filename in filenames)
    text = '<span class="overlay-text">######</span>' if overlay_text else ""
    detail = " · 图层独立循环" if animated else ""
    lines.append(f'<figure><div class="composite" '
                 f'style="width:{width}px;height:{height}px">{images}{text}</div>'
                 f'<figcaption>网页叠合预览 · {size[0]}×{size[1]}'
                 f'{detail}</figcaption></figure>')


def add_missing_layer(lines, index, child, reason):
    lines.append(f'<figure><div class="missing">{html.escape(reason)}</div>'
                 f'<figcaption>图层 [{index}]，对象 {child} · '
                 f'{html.escape(reason)}</figcaption></figure>')


def add_text_layer(lines, index, child, size):
    scale = min(1, 256 / max(size))
    width, height = (max(1, round(value * scale)) for value in size)
    lines.append(f'<figure><div class="text-placeholder" '
                 f'style="width:{width}px;height:{height}px">######</div>'
                 f'<figcaption>图层 [{index}]，对象 {child} · 文字占位'
                 '</figcaption></figure>')


def text_field_ids(sc):
    """TextField 是 40 字节 struct，开头的 u16 是对象 id。"""
    table = sc.tab.get("textfields")
    return ({struct.unpack_from("<H", table.b, p)[0]
             for p in table.struct_pos(0, 40)} if table else set())


def export(sc, out: Path, decode_ktx, max_size=1000, name_filters=()):
    """导出各图层的首帧 PNG 和浏览页。"""
    results, layers, skipped, failed = export_layers(
        sc, out, decode_ktx, max_size, name_filters, animated=False)
    return len(results), layers, skipped, failed


def animation_frames(clip):
    """MovieClip 时间轴帧数（兼容两种帧记录）。"""
    return clip.vec(8, 8)[0] or clip.vec(12, 2)[0]


def canvas_size(bounds, max_size):
    span_x, span_y = bounds[2] - bounds[0], bounds[3] - bounds[1]
    scale = min(PIXELS_PER_UNIT, max_size / max(span_x, span_y))
    return (min(max_size, max(1, math.ceil(span_x * scale))),
            min(max_size, max(1, math.ceil(span_y * scale))))


def render_frames(renderer, obj_id, count, max_size, bounds, layer=None,
                  solo=False):
    """逐帧渲染；空帧输出透明画布，不缓存整段动画。"""
    size = canvas_size(bounds, max_size)
    for frame in range(count):
        result = renderer.render(obj_id, max_size, frame, bounds, layer, solo)
        yield result[0] if result is not None else Image.new("RGBA", size)


def save_web_frames(frames, path, count, fps):
    """逐帧暂存并用 img2webp 编码；只保留当前画面和一个符号的临时帧。"""
    frames = iter(frames)
    first = next(frames)
    if count == 1:
        first.save(path, format="WEBP", quality=75, method=4)
        return
    encoder = shutil.which("img2webp")
    if encoder is None:
        raise OSError("--web-out 需要 img2webp（libwebp 工具）")
    with tempfile.TemporaryDirectory(prefix=".webp-", dir=path.parent) as temp:
        temp = Path(temp)
        commands = ["-loop 0"]
        previous, duration, seen, encoded = first, 1, 1, 0

        def write_frame(image, length):
            nonlocal encoded
            filename = f"{encoded:06d}.png"
            image.save(temp / filename)
            milliseconds = max(1, round(1000 * length / fps))
            commands.append(f"-d {milliseconds} -lossy -q 75 {filename}")
            encoded += 1

        for image in frames:
            seen += 1
            same = (image.size == previous.size and
                    ImageChops.difference(previous, image).getbbox(
                        alpha_only=False) is None)
            if same:
                duration += 1
            else:
                write_frame(previous, duration)
                previous, duration = image, 1
        if seen != count:
            raise ValueError(f"WebP 预期 {count} 帧，实际 {seen} 帧")
        write_frame(previous, duration)
        commands.append("-o result.webp")
        (temp / "args.txt").write_text("\n".join(commands) + "\n")
        result = subprocess.run([encoder, "args.txt"], cwd=temp,
                                capture_output=True, text=True, check=False)
        if result.returncode:
            raise OSError(f"img2webp 编码失败：{result.stderr.strip() or result.stdout.strip()}")
        os.replace(temp / "result.webp", path)


def export_layers(sc, out: Path, decode_ktx, max_size=1000, name_filters=(),
                  animated=False):
    """导出独立图层；PNG 取首帧，WebP 取各自的动画。"""
    if animated and shutil.which("img2webp") is None:
        raise OSError("--web-out 需要 img2webp（libwebp 工具）")
    renderer = Renderer(sc, decode_ktx)
    filters = tuple(value.casefold() for value in name_filters)
    symbols = sorted((name, obj_id) for name, obj_id in dict(sc.exports()).items()
                     if not filters or any(value in name.casefold() for value in filters))
    out.mkdir(parents=True, exist_ok=True)
    lines = preview_header("SC 符号网页预览" if animated else "SC 符号首帧预览")
    results, layer_exported, skipped, failed = [], 0, [], []
    texts = text_field_ids(sc)
    extension = "webp" if animated else "png"
    for index, (name, obj_id) in enumerate(symbols, 1):
        renderer.clip_frame_cache.clear()
        if index > 1 and (index - 1) % 100 == 0:
            print(f"渲染进度：{index - 1:,}/{len(symbols):,}", flush=True)
        group_start = None
        try:
            clips = sc.clips_by_id.get(obj_id, ())
            if not clips:
                skipped.append(name)
                continue
            visible = set(renderer.clip_appearances(clips[0]))
            layers = root_layers(sc, obj_id, visible)
            fps = clips[0].scalar(2, 1) or 30
            spans, bounds = {}, None
            for layer_index, child_id in layers:
                if child_id in texts:
                    continue
                if animated:
                    span = renderer.layer_span(obj_id, layer_index, child_id)
                else:
                    meshes = list(renderer.solo_meshes(obj_id, layer_index, 0))
                    span = ((1, renderer.bounds(meshes, obj_id)) if meshes else None)
                spans[layer_index] = span
                if span is None:
                    continue
                current = span[1]
                bounds = current if bounds is None else (
                    min(bounds[0], current[0]), min(bounds[1], current[1]),
                    max(bounds[2], current[2]), max(bounds[3], current[3]))
            if bounds is None:
                skipped.append(name)
                continue
            span_x, span_y = bounds[2] - bounds[0], bounds[3] - bounds[1]
            natural = (max(1, math.ceil(span_x * PIXELS_PER_UNIT)),
                       max(1, math.ceil(span_y * PIXELS_PER_UNIT)))
            size = canvas_size(bounds, max_size)
            layer_lines, layer_files, longest = [], [], 1
            for layer_index, child_id in layers:
                if child_id in texts:
                    add_text_layer(layer_lines, layer_index, child_id, size)
                    continue
                span = spans[layer_index]
                if span is None:
                    add_missing_layer(layer_lines, layer_index, child_id,
                                      "无可绘制网格")
                    continue
                layer_count = span[0]
                layer_name = filename_for(name, natural, size,
                                          layer_index, extension)
                layer_path = out / layer_name
                if animated:
                    child_clips = sc.clips_by_id.get(child_id, ())
                    layer_fps = ((child_clips[0].scalar(2, 1) or fps)
                                 if child_clips else fps)
                    save_web_frames(render_frames(renderer, obj_id, layer_count,
                                                  max_size, bounds, layer_index,
                                                  solo=True),
                                    layer_path, layer_count, layer_fps)
                    detail = (f"{layer_count} 帧 · {layer_fps} FPS"
                              if layer_count > 1 else "静态")
                else:
                    image = renderer.render(obj_id, max_size, bounds=bounds,
                                            layer=layer_index, solo=True)
                    image[0].save(layer_path)
                    detail = "静态"
                add_figure(layer_lines, name, layer_name,
                           f"图层 [{layer_index}] · 对象 {child_id} · {detail} · "
                           f"{size[0]}×{size[1]}")
                layer_files.append(layer_name)
                longest = max(longest, layer_count)
                layer_exported += 1
            if not layer_files:
                skipped.append(name)
                continue
            (out / filename_for(name, natural, size,
                                extension=extension)).unlink(missing_ok=True)
            results.append((name, None, longest, fps))
            group_start = start_group(lines, name)
            if len(layers) == 1 and len(layer_files) == 1:
                lines.extend(layer_lines)
            else:
                add_composite_figure(lines, name, layer_files, size,
                                     overlay_text=any(child_id in texts
                                                      for _, child_id in layers),
                                     animated=animated)
                lines.extend(layer_lines)
            end_group(lines, group_start)
            group_start = None
        except (OSError, ValueError) as exc:
            if group_start is not None:
                end_group(lines, group_start)
            failed.append((name, str(exc)))
    if symbols:
        print(f"渲染进度：{len(symbols):,}/{len(symbols):,}", flush=True)
    lines.append("</main>")
    if failed:
        lines.append("<h2>未能渲染</h2><ul>")
        for name, reason in failed:
            lines.append(f"<li>{html.escape(name)}：{html.escape(reason)}</li>")
        lines.append("</ul>")
    write_preview(out, lines)
    return results, layer_exported, skipped, failed


def export_web(sc, out: Path, decode_ktx, max_size=1000, name_filters=()):
    """导出各图层的 WebP 动画和浏览页。"""
    return export_layers(sc, out, decode_ktx, max_size, name_filters,
                         animated=True)
