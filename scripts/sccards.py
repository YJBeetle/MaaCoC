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


# 图集里一张图块可能的 8 种存法（二面体群 D4）。O 把页内像素 (U,V) 映到显示
# 坐标 (X,Y) 的线性部分 —— 符号和置换是拿一张 4x3 的标记图实测 PIL transpose
# 得到的，不是推的：例如 ROTATE_90 把 (0,0) 送到 (0,3)，即 X∝+V、Y∝-U。
ORIENTATIONS = (
    (None, np.array([[1., 0.], [0., 1.]])),
    (Image.Transpose.ROTATE_90, np.array([[0., 1.], [-1., 0.]])),
    (Image.Transpose.ROTATE_180, np.array([[-1., 0.], [0., -1.]])),
    (Image.Transpose.ROTATE_270, np.array([[0., -1.], [1., 0.]])),
    (Image.Transpose.FLIP_LEFT_RIGHT, np.array([[-1., 0.], [0., 1.]])),
    (Image.Transpose.FLIP_TOP_BOTTOM, np.array([[1., 0.], [0., -1.]])),
    (Image.Transpose.TRANSPOSE, np.array([[0., 1.], [1., 0.]])),
    (Image.Transpose.TRANSVERSE, np.array([[0., -1.], [-1., 0.]])),
)
# 拟合残差超过这个比例就说明图块不是轴对齐存放（多半是实例矩阵带了斜角），
# 宁可报出来也别硬猜一个朝向。
ORIENT_TOL = 0.01


def command_tile(sc, pages, cmd):
    """一条绘制命令 -> (显示朝向的图, 用的转置, 拟合相对残差, 面积)。

    朝向按命令单独定：每个 Shape 有自己的局部坐标系，混在一起拟合会互相污染。
    拟合必须带常数项 —— 局部坐标原点在形状中心而 uv 原点在页角，省掉截距会把
    线性部分算成一团垃圾（这个坑踩过一次）。
    """
    _flags, page, count, start = cmd
    if count <= 0 or page < 0 or page >= len(sc.tsets):
        return None, None, None, 0
    xs, ys, us, vs = np.array(sc.vertices(start, count), dtype=float).T
    w, h = sc.page(page)
    if not w or not h:
        return None, None, None, 0
    a = np.column_stack([xs, ys, np.ones_like(xs)])
    u = np.linalg.lstsq(a, us / UV * w, rcond=None)[0]
    v = np.linalg.lstsq(a, vs / UV * h, rcond=None)[0]
    k = np.array([[u[0], u[1]], [v[0], v[1]]])
    best = None
    for op, o in ORIENTATIONS:
        m = o @ k
        scale = np.trace(m) / 2
        if scale <= 0:
            continue
        err = float(np.abs(m - scale * np.eye(2)).max()) / scale
        if best is None or err < best[0]:
            best = (err, op)
    if best is None or best[0] > ORIENT_TOL:
        return None, None, best[0] if best else None, 0
    err, op = best
    box = (round(us.min() / UV * w), round(vs.min() / UV * h),
           round(us.max() / UV * w), round(vs.max() / UV * h))
    if box[2] - box[0] < 2 or box[3] - box[1] < 2:
        return None, op, err, 0
    img = pages[page].crop(box)
    if op is not None:
        img = img.transpose(op)
    return img.convert("RGBA"), op, err, (box[2] - box[0]) * (box[3] - box[1])


def face_tile(sc, pages, obj_id):
    """返回 (显示朝向的卡面图, 用的转置)。多条命令时取面积最大的一条。"""
    cmds = [c for s in leaf_shapes(sc, obj_id) for c in sc.commands(s)]
    tiles = [r for r in (command_tile(sc, pages, c) for c in cmds) if r[0] is not None]
    if not tiles:
        return None, None
    img, op, _err, _area = max(tiles, key=lambda e: e[3])
    return img, op


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
        tile, _op = face_tile(sc, pages, ex[nm])
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
        tile, op = face_tile(sc, pages, exports[sym])
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
              % (fname, sym, old.shape[1], old.shape[0],
                 " 转存%s" % getattr(op, "name", "") if op else "", tag))
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
