#!/usr/bin/env python3
"""从游戏的 .sc 里取出 符号名 -> 图集页矩形 的清单。

CoC 的卡面不是独立图片，而是 ui.sc 图集页上的一块矩形；引擎不存矩形，
它把符号解析到 Shape，再由 Shape 的绘制命令顶点的 (u,v) 包围盒算出来。
本脚本按同一套规则复现这个推导，所以每次游戏更新后可以直接重算，
不需要人工截图或比对。

    .sc = SCFILE 容器（magic "SC" + 版本 6 + FlatBuffers 描述头 + zstd 正文）
    正文 = 连续的 [u32 长度][flatbuffers 分块]
           Resources / Exports / TextFields / Shapes / MovieClips /
           Modifiers / TextureSets
    矩形 = 符号对应 Shape 每条命令的顶点 (u,v) 包围盒，
           u,v 是 16 位归一化值，乘页宽高即像素。

用法：
    python3 scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc -o ui.frames.json
    python3 scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --list 'icon_unit_*'
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import struct
import sys
from pathlib import Path

import zstandard

CHUNK_ORDER = ["resources", "exports", "textfields", "shapes",
               "movieclips", "modifiers", "texturesets"]
ZSTD_FRAME = b"\x28\xb5\x2f\xfd"
UV_SCALE = 65536.0          # 顶点 uv 是 16 位归一化坐标


def u16(b, p):
    return struct.unpack_from("<H", b, p)[0]


def u32(b, p):
    return struct.unpack_from("<I", b, p)[0]


def i32(b, p):
    return struct.unpack_from("<i", b, p)[0]


class Table:
    """flatbuffers table。b 是分块字节，p 是 table 在其中的位置。"""

    def __init__(self, b, p):
        self.b = b
        self.p = p
        self.vt = p - i32(b, p)

    def slot(self, field):
        off = 4 + 2 * field
        if off >= u16(self.b, self.vt):
            return None
        value = u16(self.b, self.vt + off)
        return value or None

    def at(self, field):
        s = self.slot(field)
        return None if s is None else self.p + s

    def scalar(self, field, size=4, default=0):
        a = self.at(field)
        if a is None:
            return default
        return struct.unpack_from({1: "<B", 2: "<H", 4: "<I"}[size], self.b, a)[0]

    def vec(self, field, item_size=1):
        """向量字段 -> (元素个数, 数据位置)。"""
        a = self.at(field)
        if a is None:
            return 0, None
        v = a + u32(self.b, a)
        if v + 4 > len(self.b):
            raise ValueError("向量长度越界（字段 %d）" % field)
        n = u32(self.b, v)
        if n > (len(self.b) - v - 4) // item_size:
            raise ValueError("向量内容越界（字段 %d，元素大小 %d）" % (field, item_size))
        return n, v + 4

    def strings(self, field):
        n, v = self.vec(field, 4)
        out = []
        for i in range(n):
            o = v + 4 * i
            p = o + u32(self.b, o)
            out.append(self.b[p + 4:p + 4 + u32(self.b, p)].decode("utf-8", "replace"))
        return out

    def tables(self, field):
        n, v = self.vec(field, 4)
        out = []
        for i in range(n):
            o = v + 4 * i
            p = o + u32(self.b, o)
            if 0 < p < len(self.b) - 4 and 0 <= p - i32(self.b, p) < len(self.b):
                out.append(Table(self.b, p))
        return out

    def table(self, field):
        a = self.at(field)
        if a is None:
            return None
        p = a + u32(self.b, a)
        if not 0 < p < len(self.b) - 4:
            return None
        vt = p - i32(self.b, p)
        return Table(self.b, p) if 0 <= vt < len(self.b) - 4 else None

    def struct_pos(self, field, size):
        n, v = self.vec(field, size)
        return [v + size * i for i in range(n)]


def root(b):
    return Table(b, u32(b, 0))


def scfile_body(data: bytes) -> tuple[bytes, dict[str, int]]:
    """按描述头长度定位 zstd 帧，返回解压正文与容器信息。"""
    if len(data) < 12 or data[:2] != b"SC":
        raise ValueError("不是 SCFILE（缺 SC 魔数）")
    version = u32(data, 2)
    if version != 6:
        raise ValueError("仅支持 SCFILE v6，当前版本为 %d" % version)
    header_size = u32(data, 8)
    frame_at = 12 + header_size
    if header_size < 8 or frame_at > len(data):
        raise ValueError("SCFILE 描述头长度越界：%d" % header_size)
    try:
        header = root(data[12:frame_at])
        metadata_count, _ = header.vec(10, 4)
        compressed_size = header.scalar(11)
    except (ValueError, struct.error) as exc:
        raise ValueError("SCFILE 描述头无效") from exc
    frame_end = frame_at + (compressed_size or len(data) - frame_at)
    if frame_end > len(data) or data[frame_at:frame_at + 4] != ZSTD_FRAME:
        raise ValueError("SCFILE 描述头指向的 zstd 帧无效")
    try:
        decoder = zstandard.ZstdDecompressor().decompressobj()
        blob = decoder.decompress(memoryview(data)[frame_at:frame_end])
    except zstandard.ZstdError as exc:
        raise ValueError("SCFILE 正文解压失败") from exc
    if not decoder.eof or len(blob) < 8:
        raise ValueError("SCFILE zstd 帧不完整")
    return blob, {"version": version, "header_size": header_size,
                  "metadata_count": metadata_count}


def split_chunks(blob):
    pos, out = 0, []
    while pos + 4 <= len(blob):
        ln = u32(blob, pos)
        if ln < 8 or pos + 4 + ln > len(blob):
            raise ValueError("flatbuffers 分块长度无效，偏移 %d，长度 %d" % (pos, ln))
        out.append((pos + 4, ln))
        pos += 4 + ln
    if pos != len(blob):
        raise ValueError("flatbuffers 正文末尾有 %d 个多余字节" % (len(blob) - pos))
    return out


class ScFile:
    """一个 .sc：符号名、形状、纹理页。"""

    def __init__(self, data: bytes):
        blob, self.container = scfile_body(data) if data[:2] == b"SC" else (data, None)
        self.blob = blob
        parts, tab, off = [], {}, {}
        for i, (o, ln) in enumerate(split_chunks(blob)):
            name = CHUNK_ORDER[i] if i < len(CHUNK_ORDER) else "chunk%d" % i
            parts.append((name, ln))
            tab[name] = root(blob[o:o + ln])
            off[name] = o
        if "resources" not in tab or "exports" not in tab:
            raise ValueError("正文不是 flatbuffers 分块结构，分块=%s" % parts)
        self.parts = parts
        self.tab = tab
        self.off = off
        r = tab["resources"]
        self.strings = r.strings(0)
        # 分块内的 Table 用的是切片坐标，加回分块起点才是正文绝对偏移
        n, v = r.vec(5)
        if v is None:
            raise ValueError("Resources 缺少 shape_points")
        # shape_points 是字节向量；每个顶点占 12 字节。
        if n % 12:
            raise ValueError("Resources.shape_points 长度不是 12 的倍数")
        self.points_n, self.points_at = n // 12, off["resources"] + v
        ex = tab["exports"]
        self.export_ids = [u16(ex.b, p) for p in ex.struct_pos(0, 2)]
        self.export_names = [u32(ex.b, p) for p in ex.struct_pos(1, 4)]
        if len(self.export_ids) != len(self.export_names):
            raise ValueError("Exports 的对象 id 与名字数量不一致")
        self.shapes = tab["shapes"].tables(0) if "shapes" in tab else []
        self.clips = tab["movieclips"].tables(0) if "movieclips" in tab else []
        self.tsets = tab["texturesets"].tables(0) if "texturesets" in tab else []
        self._shapes_by_id = None
        self._clips_by_id = None
        self._pages = None

    # ---- 纹理页 -------------------------------------------------------
    def page(self, index):
        """(宽, 高)；highres 存在时优先。"""
        if self._pages is None:
            self._pages = []
            for i in range(len(self.tsets)):
                best = (0, 0)
                for f in (0, 1):
                    t = self.tsets[i].table(f)
                    if t is None:
                        continue
                    w, h = t.scalar(2, 2), t.scalar(3, 2)
                    if w and h:
                        best = (w, h)
                self._pages.append(best)
        return self._pages[index]

    def page_ktx(self, index):
        """该页的 KTX 原始字节，用于确认页号和解码出的图一一对应。"""
        st = self.tsets[index]
        for f in (1, 0):
            t = st.table(f)
            if t is None:
                continue
            n, v = t.vec(4)
            if n:
                return t.b[v:v + n]
        return None

    # ---- 几何 ---------------------------------------------------------
    def vertices(self, start, count):
        """ShapePoint{x, y 为 f32；u, v 为 u16}，12 字节一个。"""
        if start < 0 or count < 0 or start + count > self.points_n:
            raise ValueError("顶点范围越界：起点 %d，数量 %d，总数 %d" %
                             (start, count, self.points_n))
        base = self.points_at + 12 * start
        return [struct.unpack_from("<2f2H", self.blob, base + 12 * i) for i in range(count)]

    def commands(self, shape):
        """ShapeDrawBitmapCommand{flags, 页号, 顶点数, 起始顶点下标}。"""
        return [struct.unpack_from("<4i", shape.b, p) for p in shape.struct_pos(1, 16)]

    @property
    def shapes_by_id(self):
        if self._shapes_by_id is None:
            m = {}
            for s in self.shapes:
                m.setdefault(s.scalar(0, 2), []).append(s)
            self._shapes_by_id = m
        return self._shapes_by_id

    @property
    def clips_by_id(self):
        if self._clips_by_id is None:
            m = {}
            for c in self.clips:
                m.setdefault(c.scalar(0, 2), []).append(c)
            self._clips_by_id = m
        return self._clips_by_id

    def boxes(self, obj_id, seen=None):
        """一个对象 id 覆盖到的图集矩形（页号 + 像素坐标，未取整）。"""
        seen = seen if seen is not None else set()
        out = []
        for s in self.shapes_by_id.get(obj_id, ()):
            if id(s) in seen:
                continue
            seen.add(id(s))
            for _flags, page, count, start in self.commands(s):
                if count <= 0 or page < 0 or page >= len(self.tsets):
                    raise ValueError("对象 %d 的绘制命令无效：页 %d，顶点数 %d" %
                                     (obj_id, page, count))
                w, h = self.page(page)
                if not (w and h):
                    raise ValueError("纹理页 %d 的尺寸无效" % page)
                pts = self.vertices(start, count)
                out.append((page,
                            min(p[2] for p in pts) / UV_SCALE * w,
                            min(p[3] for p in pts) / UV_SCALE * h,
                            max(p[2] for p in pts) / UV_SCALE * w,
                            max(p[3] for p in pts) / UV_SCALE * h))
        for c in self.clips_by_id.get(obj_id, ()):
            if id(c) in seen:
                continue
            seen.add(id(c))
            for p in c.struct_pos(5, 2):
                out += self.boxes(u16(c.b, p), seen)
        return out

    def exports(self):
        """符号名 -> 对象 id（同名可多次导出）。"""
        for i in range(len(self.export_ids)):
            idx = self.export_names[i]
            if 0 <= idx < len(self.strings):
                yield self.strings[idx], self.export_ids[i]


def frame_table(sc: ScFile, merge=True):
    """符号名 -> [{page, x, y, w, h}]。merge 时同一页上的多块合成一个外接框。"""
    out = {}
    for name, oid in sc.exports():
        boxes = [b for b in sc.boxes(oid) if b[3] > b[1] and b[4] > b[2]]
        if not boxes:
            continue
        if not merge:
            out.setdefault(name, []).extend(
                {"page": b[0], "x": round(b[1]), "y": round(b[2]),
                 "w": round(b[3] - b[1]), "h": round(b[4] - b[2])} for b in boxes)
            continue
        bypage = {}
        for b in boxes:
            bypage.setdefault(b[0], []).append(b)
        for page, rows in bypage.items():
            x0 = round(min(b[1] for b in rows))
            y0 = round(min(b[2] for b in rows))
            x1 = round(max(b[3] for b in rows))
            y1 = round(max(b[4] for b in rows))
            out.setdefault(name, []).append(
                {"page": page, "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0})
    return out


def selfcheck(sc: ScFile, frames):
    """结构自检：矩形必须落在自己页的尺寸内。返回问题列表。"""
    bad = []
    for name, rows in frames.items():
        for r in rows:
            w, h = sc.page(r["page"])
            if r["w"] <= 0 or r["h"] <= 0 or r["x"] < 0 or r["y"] < 0 \
                    or r["x"] + r["w"] > w or r["y"] + r["h"] > h:
                bad.append((name, r, (w, h)))
    return bad


def load_sc(source):
    """读取一个已解压的 SCFILE 文件。"""
    try:
        data = Path(source).read_bytes()
    except FileNotFoundError as exc:
        raise ValueError("找不到 .sc 文件：%s" % source) from exc
    if not data.startswith(b"SC"):
        raise ValueError("不是 SCFILE 文件：%s" % source)
    return data


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("source", type=Path, help="已解压的 .sc 文件")
    ap.add_argument("-o", "--output", type=Path, help="写出 JSON 清单")
    ap.add_argument("--list", metavar="GLOB", default="*", help="只打印匹配的符号")
    ap.add_argument("--split", action="store_true", help="同名不合并，逐块列出")
    args = ap.parse_args(argv)

    try:
        sc = ScFile(load_sc(args.source))
    except (OSError, ValueError) as exc:
        ap.exit(1, "%s: %s\n" % (ap.prog, exc))
    frames = frame_table(sc, merge=not args.split)
    pages = [sc.page(i) for i in range(len(sc.tsets))]
    if sc.container:
        print("SCFILE v%d" % sc.container["version"])
        print("├─ FlatBuffers 描述头：{:,} 字节，符号元数据 {:,} 条".format(
              sc.container["header_size"], sc.container["metadata_count"]))
    else:
        print("SCFILE 正文")
    print("└─ 正文（解压后 {:,} 字节；分块偏移相对正文起点）".format(len(sc.blob)))
    for i, (name, size) in enumerate(sc.parts):
        last = i == len(sc.parts) - 1
        print("   %s [%d] %-12s 偏移 %10s，长度 %10s 字节" %
              ("└─" if last else "├─", i, name,
               format(sc.off[name], ","), format(size, ",")))
        if name == "texturesets":
            indent = "      " if last else "   │  "
            for page, (w, h) in enumerate(pages):
                branch = "└─" if page == len(pages) - 1 else "├─"
                print("%s%s 页 [%d]：%d × %d" % (indent, branch, page, w, h))
    print("有矩形的符号 %d 个，推导出的矩形 %d 个" %
          (len(frames), sum(len(v) for v in frames.values())))
    bad = selfcheck(sc, frames)
    print("越界自检: %d 处异常" % len(bad))
    for row in bad[:5]:
        print("   ", row)
    if bad:
        return 1

    shown = 0
    for name in sorted(frames):
        if not fnmatch.fnmatchcase(name, args.list):
            continue
        shown += 1
        if shown <= 400:
            print("  %-40s %s" % (name, frames[name]))
    if shown > 400:
        print("  ...共 %d 个，仅显示前 400" % shown)

    if args.output:
        args.output.write_text(json.dumps(
            {"source": str(args.source),
             "pages": [{"w": w, "h": h} for w, h in pages],
             "frames": frames}, ensure_ascii=False, indent=1))
        print("写出 %s" % args.output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
