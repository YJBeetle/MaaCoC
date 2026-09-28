#!/usr/bin/env python3
"""游戏更新后，把 .sc 里的全部卡面图块导出来，索引用官方符号名。

卡面不是独立图片，而是 ui.sc 图集页上的一块矩形，而且游戏不存这个矩形 ——
它存矢量形状，矩形是顶点 uv 包围盒乘页尺寸算出来的。推导链在 scframes.py。

    python3 scripts/refresh-cards/sccards.py <APK 或 ui.sc>

产出 assets/image/cards/<符号名>.png 加一份 assets/config/cards.json 索引。
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import os
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image

# 同级拿 scframes，上一级 scripts/ 拿共用的 KTX 解码 sctx2png
HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent)]
import scframes          # noqa: E402
import sctx2png          # noqa: E402

REPO = HERE.parents[1]
UV = 65536.0
DEFAULT_PREFIXES = ("icon_unit_", "icon_spell_")


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


# 图块在图集里可能是 8 种存法之一（二面体群 D4）。O 把页内像素 (U,V) 映到显示
# 坐标 (X,Y) 的线性部分 —— 符号和置换是拿一张 4x3 标记图实测 PIL transpose 得到
# 的，不是推的：ROTATE_90 把 (0,0) 送到 (0,3)，即 X∝+V、Y∝-U。
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
# 残差超过这个比例说明图块不是轴对齐存放（多半是实例矩阵带了斜角），
# 宁可报出来也别硬猜一个朝向。
ORIENT_TOL = 0.01


def uv_box(us, vs, w, h):
    """uv 包围盒 -> 页内像素矩形。"""
    return (round(us.min() / UV * w), round(vs.min() / UV * h),
            round(us.max() / UV * w), round(vs.max() / UV * h))


def command_tile(sc, pages, cmd):
    """一条绘制命令 -> (显示朝向的图, 用的转置, 像素矩形, 面积)。

    朝向必须按命令单独定：卡面 MovieClip 的孩子混着遮罩、脸和背景块，各有各的
    局部坐标系，混在一起拟合会互相污染。拟合还必须带常数项 —— 局部原点在形状
    中心而 uv 原点在页角，省掉截距会把线性部分算成一团垃圾（这坑踩过）。
    """
    _flags, page, count, start = cmd
    if count <= 0 or page < 0 or page >= len(sc.tsets):
        raise ValueError("绘制命令无效：页 %d，顶点数 %d" % (page, count))
    xs, ys, us, vs = np.array(sc.vertices(start, count), dtype=float).T
    w, h = sc.page(page)
    if not w or not h:
        raise ValueError("纹理页 %d 的尺寸无效" % page)
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
    _err, op = best
    x0, y0, x1, y1 = uv_box(us, vs, w, h)
    if not (0 <= x0 <= x1 <= w and 0 <= y0 <= y1 <= h):
        raise ValueError("纹理页 %d 的裁剪矩形越界：%s" %
                         (page, (x0, y0, x1, y1)))
    if x1 - x0 < 2 or y1 - y0 < 2:
        return None, op, None, 0
    img = pages[page].crop((x0, y0, x1, y1))
    if op is not None:
        img = img.transpose(op)
    return img.convert("RGBA"), op, [page, x0, y0, x1 - x0, y1 - y0], (x1 - x0) * (y1 - y0)


def biggest_tile(sc, pages, obj_id):
    """对象所有绘制命令里面积最大的那块 —— 卡面就是一块主图，其余是遮罩/背景。"""
    tiles = [t for t in (command_tile(sc, pages, c)
                         for s in leaf_shapes(sc, obj_id) for c in sc.commands(s))
             if t[0] is not None]
    if not tiles:
        return None, None, None
    img, op, rect, _area = max(tiles, key=lambda e: e[3])
    return img, op, rect


def clip_of(sc, obj_id):
    """卡面 MovieClip 用的裁剪遮罩对象 id（既不是 Shape 也不是 MovieClip 的孩子）。"""
    for c in sc.clips_by_id.get(obj_id, ()):
        for p in c.struct_pos(5, 2):
            kid = struct.unpack_from("<H", c.b, p)[0]
            if kid not in sc.shapes_by_id and kid not in sc.clips_by_id:
                return kid
    return None


def kind(name, all_names):
    """按官方命名规则给卡面分类。

    超级兵不能只看 elite_ 前缀：21 个 icon_unit_elite_* 里 7 个压根没有同名普通版
    （bowler / hogrider / minion / valkyrie / icehound / infernodragon /
    barbarian_group_cc），那里 elite_ 就是本体图块的名字。
    所以判据是「elite_ 且有同名普通版」。
    """
    if name.startswith("icon_spell_"):
        return "spell"
    if name.startswith("icon_unit_pet_"):
        return "pet"
    if name.startswith("icon_unit_siege_machine"):
        return "siege"
    if name.startswith("icon_hero"):
        return "hero"
    if name.startswith("icon_gear"):
        return "gear"
    if name.startswith("icon_2025"):
        return "event"
    if name.startswith("icon_league"):
        return "league"
    if name.startswith("icon_unit_elite_"):
        base = name.replace("icon_unit_elite_", "icon_unit_").replace("_cc", "")
        return "super" if base in all_names else "troop"
    return "troop"


def export(sc, pages, prefixes, out, manifest):
    """导出给定前缀下的全部卡面，并写符号名到文件名的索引。"""
    ex = dict(sc.exports())
    all_names = set(ex)
    selected = sorted(n for n in ex if n.startswith(tuple(prefixes)))
    if not selected:
        raise ValueError("没有匹配前缀的符号：%s" % ", ".join(prefixes))
    folded = collections.Counter(n.casefold() for n in selected)
    rows, images, failed = {}, {}, []
    for name in selected:
        if Path(name).name != name or name in (".", ".."):
            raise ValueError("符号名不能用作文件名：%r" % name)
        img, op, rect = biggest_tile(sc, pages, ex[name])
        if img is None:
            failed.append(name)
            continue
        filename = name + ".png"
        if folded[name.casefold()] > 1:
            filename = name + "--" + hashlib.sha256(name.encode()).hexdigest()[:8] + ".png"
        images[filename] = img
        rows[name] = {"kind": kind(name, all_names), "clip": clip_of(sc, ex[name]),
                      "orient": getattr(op, "name", "none"),
                      "size": list(img.size), "rect": rect}
        if filename != name + ".png":
            rows[name]["file"] = filename
    if failed:
        raise ValueError("裁不出 %d 个卡面：%s" % (len(failed), ", ".join(failed)))

    old_files = set()
    if manifest.exists():
        old_files = {row.get("file", name + ".png")
                     for name, row in json.loads(manifest.read_text())["cards"].items()}
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".cards-", dir=out.parent) as stage_name:
        stage = Path(stage_name)
        for filename, img in images.items():
            img.save(stage / filename)
        out.mkdir(parents=True, exist_ok=True)
        for filename in images:
            os.replace(stage / filename, out / filename)

    manifest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", prefix=".cards-",
                                     dir=manifest.parent, delete=False) as tmp:
        tmp.write(json.dumps({"prefixes": list(prefixes), "cards": rows},
                             ensure_ascii=False, indent=1) + "\n")
    os.replace(tmp.name, manifest)
    for filename in old_files - images.keys():
        if Path(filename).name == filename and filename not in (".", ".."):
            (out / filename).unlink(missing_ok=True)
    print("导出 %d 张 -> %s" % (len(rows), out))
    print("类别: %s" % dict(collections.Counter(r["kind"] for r in rows.values()).most_common()))
    print("索引 -> %s" % manifest)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("source", type=Path, help="ui.sc / 含 ui.sc 的 APK 或目录")
    ap.add_argument("--sc", default="ui.sc", help="从 APK/目录里挑哪个 .sc")
    ap.add_argument("--prefix", action="append",
                    help="要导出的符号前缀，可重复（默认 %s）" % " 和 ".join(DEFAULT_PREFIXES))
    ap.add_argument("--out", type=Path, default=REPO / "assets/image/cards")
    ap.add_argument("--manifest", type=Path, default=REPO / "assets/config/cards.json")
    args = ap.parse_args(argv)

    sc = scframes.ScFile(scframes.load_sc(args.source, args.sc))
    pages = [sctx2png.ktx_image(sc.page_ktx(i)) for i in range(len(sc.tsets))]
    for i, page in enumerate(pages):
        if page is None or page.size != sc.page(i):
            raise ValueError("纹理页 %d 解码尺寸与 .sc 记录不一致" % i)
    export(sc, pages, args.prefix or list(DEFAULT_PREFIXES), args.out, args.manifest)
    return 0


if __name__ == "__main__":
    sys.exit(main())
