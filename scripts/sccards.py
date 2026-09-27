#!/usr/bin/env python3
"""游戏更新后，用 .sc 里的原始图块刷新战斗条卡牌模板。

卡面不是独立图片，而是 ui.sc 图集页上的一块矩形，位置由
scripts/scframes.py 从 flatbuffers 里推出来。本脚本把那块矩形裁出来、
按模板原有尺寸重新装框、再把模板里原有的纯绿 (0,255,0) 角标遮罩
原样贴回去（green_mask 靠它跳过会变化的等级/费用角标）。

模板的几何（尺寸、绿块位置）一律沿用现存的 PNG —— 要换的只有像素。

用法：
    python3 scripts/sccards.py ui.sc                 # 干跑，报告差异
    python3 scripts/sccards.py ui.sc --apply         # 写盘
    python3 scripts/sccards.py ui.sc --symbols auto  # 重新生成符号对照表
"""

from __future__ import annotations

import argparse
import json
import re
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import scframes          # noqa: E402
import sctx2png          # noqa: E402

REPO = Path(__file__).resolve().parents[1]
GREEN = (0, 255, 0)
UV = 65536.0


def norm(s):
    return re.sub(r"[^a-z]", "", s.lower())


def leaf_shapes(sc, obj_id, seen=None):
    """对象及其子 MovieClip 下的全部 Shape。"""
    seen = seen if seen is not None else set()
    out = []
    for s in sc.shapes_by_id.get(obj_id, ()):
        if id(s) not in seen:
            seen.add(id(s))
            out.append(s)
    for c in sc.clips_by_id.get(obj_id, ()):
        if id(c) in seen:
            continue
        seen.add(id(c))
        for p in c.struct_pos(5, 2):
            out += leaf_shapes(sc, struct.unpack_from("<H", c.b, p)[0], seen)
    return out


def face_tile(sc, pages, obj_id):
    """返回 (显示朝向的卡面图, 是否被转存 90 度)。

    朝向不靠试出来：把顶点的局部 (x,y) 分别对 uv 做最小二乘，
    u 跟着 y 走就是图集里转存过。
    """
    pts, page = [], None
    for s in leaf_shapes(sc, obj_id):
        for _flags, tex, count, start in sc.commands(s):
            if count <= 0:
                continue
            pts += [tuple(v) for v in sc.vertices(start, count)]
            page = tex
    if not pts or page is None:
        return None, False
    xs, ys, us, vs = np.array(pts, dtype=float).T

    def resid(src, dst):
        a = np.vstack([src, np.ones_like(src)]).T
        k, *_ = np.linalg.lstsq(a, dst, rcond=None)
        return float(np.abs(dst - a @ k).max())

    rotated = resid(ys, us) < resid(xs, us)
    w, h = sc.page(page)
    box = (round(us.min() / UV * w), round(vs.min() / UV * h),
           round(us.max() / UV * w), round(vs.max() / UV * h))
    img = pages[page].crop(box)
    if rotated:
        img = img.transpose(Image.ROTATE_90)
    return img.convert("RGBA"), rotated


def fit_cover(src, width, height):
    """等比放大到铺满窗口后居中裁切。"""
    s = max(width / src.width, height / src.height)
    w, h = max(width, round(src.width * s)), max(height, round(src.height * s))
    im = src.resize((w, h), Image.LANCZOS)
    return im.crop(((w - width) // 2, (h - height) // 2,
                    (w - width) // 2 + width, (h - height) // 2 + height))


def build_symbols(sc, templates):
    """按 icon_unit_<小写名> 自动对一遍符号，对不上的留空等人工补。"""
    names = {n for n, _ in sc.exports()}
    out = {}
    for p in templates:
        key = norm(re.sub(r"^\d+_", "", p.stem))
        guess = "icon_unit_" + key
        if guess in names:
            out[p.name] = guess
            continue
        cand = sorted(n for n in names
                      if n.startswith("icon_unit_") and norm(n[10:]) == key)
        out[p.name] = cand[0] if len(cand) == 1 else None
    return out


def review_sheet(sc, pages, out, cols=10, cell=132):
    """把所有 icon_unit_* 图块铺成一张带名字的对照表，给人挑符号用。"""
    names = sorted({n for n, _ in sc.exports() if n.startswith("icon_unit_")})
    sheet = Image.new("RGBA", (cols * cell, cell * (len(names) // cols + 1)),
                      (26, 26, 30, 255))
    dr = ImageDraw.Draw(sheet)
    ex = dict(sc.exports())
    for i, nm in enumerate(names):
        tile, _rot = face_tile(sc, pages, ex[nm])
        if tile is None:
            continue
        tile.thumbnail((cell - 4, cell - 14))
        x, y = (i % cols) * cell, (i // cols) * cell
        sheet.paste(tile, (x, y + 12), tile)
        dr.text((x + 2, y + 1), nm[10:], fill=(255, 255, 0, 255))
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out)
    print("写出符号对照图 %s（%d 个 icon_unit_*）" % (out, len(names)))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("source", type=Path, help="ui.sc（SCFILE 容器或已解压正文）")
    ap.add_argument("--templates", type=Path, default=REPO / "assets/image/Soldier")
    ap.add_argument("--symbols", type=Path,
                    default=REPO / "assets/config/card-symbols.json")
    ap.add_argument("--apply", action="store_true", help="写盘，默认只报告")
    ap.add_argument("--auto", action="store_true", help="重写符号对照表")
    ap.add_argument("--review", type=Path, help="导出 icon_unit_* 对照图后退出")
    args = ap.parse_args(argv)

    templates = sorted(args.templates.glob("*.png"))
    sc = scframes.ScFile(args.source.read_bytes())
    pages = [sctx2png.ktx_image(sc.page_ktx(i)) for i in range(len(sc.tsets))]
    if args.review:
        review_sheet(sc, pages, args.review)
        return 0
    exports = dict(sc.exports())
    if args.auto or not args.symbols.exists():
        table = build_symbols(sc, templates)
        args.symbols.write_text(json.dumps(table, ensure_ascii=False, indent=1) + "\n")
        left = [k for k, v in table.items() if not v]
        print("写出符号对照表 %s（%d/%d 自动对上）"
              % (args.symbols, len(table) - len(left), len(table)))
        if left:
            print("待人工指定:", ", ".join(left))
        if args.auto and not args.apply:
            return 0
    else:
        table = json.loads(args.symbols.read_text())

    print("页: %s" % [p.size for p in pages])
    same = changed = miss = 0
    for fname, sym in sorted(table.items()):
        path = args.templates / fname
        if not path.exists():
            print("  %-26s 模板文件不存在" % fname)
            miss += 1
            continue
        old = np.array(Image.open(path).convert("RGBA"))
        if not sym or sym not in exports:
            print("  %-26s 符号未对照%s" % (fname, "" if sym else "上"))
            miss += 1
            continue
        tile, rotated = face_tile(sc, pages, exports[sym])
        if tile is None:
            print("  %-26s %-26s 没有图块" % (fname, sym))
            miss += 1
            continue
        new = np.array(fit_cover(tile, old.shape[1], old.shape[0]))
        new[:, :, 3] = 255
        mask = np.all(old[:, :, :3] == np.array(GREEN), axis=-1)   # 角标遮罩原样保留
        new[mask] = (GREEN[0], GREEN[1], GREEN[2], 255)
        diff = int(np.abs(old[:, :, :3].astype(int) - new[:, :, :3].astype(int)).mean())
        tag = "不变" if diff == 0 else "差异 %d" % diff
        print("  %-26s %-28s %dx%d%s  %s"
              % (fname, sym, old.shape[1], old.shape[0], " 转存" if rotated else "", tag))
        if diff:
            changed += 1
            if args.apply:
                Image.fromarray(new, "RGBA").save(path)
        else:
            same += 1
    print("\n一致 %d，需更新 %d，未覆盖 %d%s"
          % (same, changed, miss, "（已写盘）" if args.apply else "（干跑，未写盘）"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
