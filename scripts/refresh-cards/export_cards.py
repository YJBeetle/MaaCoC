#!/usr/bin/env python3
"""从 .sc 合成供 MAAFW 匹配的卡牌模板。

    python3 scripts/refresh-cards/export_cards.py var/coc-unpack/sc/ui.sc

产出 assets/image/Cards/{Soldier,Hero,Spell}/<卡牌名>.png。
"""

from __future__ import annotations

import argparse
import math
import os
import sys
import tempfile
from pathlib import Path
from typing import NamedTuple

from PIL import Image, ImageDraw, ImageOps

# 同级拿 scframes，上一级 scripts/ 拿共用的 KTX 解码 sctx2png
HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent)]
import scframes          # noqa: E402
import scrender          # noqa: E402
import sctx2png          # noqa: E402

REPO = HERE.parents[1]

# 卡牌名、SC 符号、可选的卡面图层，以及对应卡底的图层。
# Soldier 沿用原模板的编号前缀，方便按兵种顺序浏览。
class CardSpec(NamedTuple):
    name: str
    symbol: str
    image_layer: int | None = None
    background_layer: int = 0
    extra_masks: tuple[tuple[float, float, float, float], ...] = ()


SOLDIER_CARDS = (
    CardSpec("0_Barbarian", "icon_unit_barbarian"),
    CardSpec("1_Archer", "icon_unit_archer"),
    CardSpec("3_Giant", "icon_unit_giant"),
    CardSpec("4_Goblin", "icon_unit_goblin"),
    CardSpec("5_Breaker", "icon_unit_wallbreaker"),
    CardSpec("6_Balloon", "icon_unit_balloon"),
    CardSpec("7_Wizard", "icon_unit_wizard"),
    CardSpec("8_Healer", "icon_unit_healer"),
    CardSpec("9_Dragon", "icon_unit_dragon"),
    CardSpec("10_P.E.K.K.A", "icon_unit_pekka"),
    CardSpec("11_BabyDragon", "icon_unit_babydragon"),
    CardSpec("12_Miner", "icon_unit_miner"),
    CardSpec("13_ElectroDragon", "icon_unit_lightningDragon", image_layer=1),
    CardSpec("14_Yeti", "icon_unit_yeti"),
    CardSpec("15_DragonRider", "icon_unit_dragon_rider"),
    CardSpec("17_RootRider", "icon_unit_root_rider"),
    CardSpec("18_Thrower", "icon_unit_thrower"),
    CardSpec("50_Minion", "icon_unit_gargoyle"),
    CardSpec("51_HogRider", "icon_unit_boarRider"),
    CardSpec("52_Valkyrie", "icon_unit_warriorGirl"),
    CardSpec("53_Golen", "icon_unit_golem"),
    CardSpec("54_Witch", "icon_unit_witch"),
    CardSpec("55_LavaHound", "icon_unit_tiny"),
    CardSpec("56_Bowler", "icon_unit_troll"),
    CardSpec("57_IceGolem", "icon_unit_iceGolem"),
    CardSpec("58_Headhunter", "icon_unit_headhunter"),
    CardSpec("59_ApprenticeWarden", "icon_unit_apprentice"),
    CardSpec("60_Druid", "icon_unit_druid_bear"),
    CardSpec("61_Furnace", "icon_unit_furnace"),
    CardSpec("100_Broom", "icon_unit_majo"),
    CardSpec("101_BarbarianKicker", "icon_unit_footballbarbarian"),
    CardSpec("102_M.E.C.H.A", "icon_unit_mecha"),
    CardSpec("103_BattleRam", "icon_unit_battleram_cc"),
    CardSpec("104_IceWizard", "icon_unit_icewizard"),
    CardSpec("105_DebtCollector", "icon_unit_goblin_tax_collector"),
    CardSpec("106_GiantThrower", "icon_unit_footballgiant"),
    CardSpec("107_Firecracker", "icon_unit_firecracker"),
    CardSpec("108_RamRider", "icon_unit_cookie_ramrider"),
    CardSpec("109_PartyWizard", "icon_unit_partyWizard"),
    CardSpec("110_MeteorGolem", "icon_unit_splitgolem"),
    CardSpec("111_IceMinion", "icon_unit_ice_minion"),
    CardSpec("112_Lavaloon", "icon_unit_lavaloon"),
    CardSpec("113_Barcher", "icon_unit_barcher"),
    CardSpec("SuperBalloon", "icon_unit_elite_balloon", background_layer=1),
    CardSpec("SuperDragon", "icon_unit_elite_dragon", background_layer=1),
    CardSpec("SuperMiner", "icon_unit_elite_miner", background_layer=1),
    CardSpec("SuperWitch", "icon_unit_elite_witch", background_layer=1),
)

# Soldier 卡面合成时使用 capacity_slot 的背景图层。
SOLDIER_BACKGROUND = "capacity_slot"

HERO_CARDS = (
    CardSpec("King", "icon_hero_barbarianKing"),
    CardSpec("Queen", "icon_hero_archerQueen"),
    # 咏王右下角的飞行/地面模式切换开关是其他英雄没有的动态区域。
    CardSpec("Warden", "icon_hero_grandwarden",
             extra_masks=((0.39, 0.67, 1.0, 0.96),)),
    CardSpec("Prince", "icon_hero_minionprince"),
    CardSpec("Mars", "icon_hero_warriorPrincess"),
)
SPELL_CARDS = (
    CardSpec("SpellRage", "icon_spell_rage"),
    CardSpec("SpellSpeed", "icon_spell_speedup"),
)
# 卡面在各卡底中的位置，比例相对各自的整张卡。
SOLDIER_FACE_BOX = (3 / 71, 22 / 94, 68 / 71, 91 / 94)
HERO_FACE_BOX = (2 / 70, 2 / 92, 68 / 70, 90 / 92)
SPELL_FACE_BOX = (2 / 70, 20 / 92, 68 / 70, 90 / 92)
# locked 英雄卡底的原始画布为 84×112，实机卡牌比直接乘统一倍率更小。
HERO_SOURCE_SCALE = 7 / 8
# 英雄肖像相对卡底向上移动 5 个 SC 画布像素，卡底及裁切范围不动。
HERO_FACE_Y_OFFSET = -5
# 数量、等级等动态内容由 MAAFW 的 green_mask 跳过。
SOLDIER_MASKS = ((0.39, 0.02, 0.95, 0.21), (0.04, 0.68, 0.42, 0.95))
HERO_MASKS = ((0.02, 0.04, 0.43, 0.36), (0.02, 0.68, 0.37, 0.96))
SPELL_MASKS = ((0.62, 0.02, 0.96, 0.21), (0.04, 0.71, 0.36, 0.94))
CARD_GROUPS = (
    ("Soldier", SOLDIER_CARDS, SOLDIER_BACKGROUND, SOLDIER_FACE_BOX, SOLDIER_MASKS, 1.0, 0),
    ("Hero", HERO_CARDS, "capacity_slot_hero_locked", HERO_FACE_BOX, HERO_MASKS,
     HERO_SOURCE_SCALE, HERO_FACE_Y_OFFSET),
    ("Spell", SPELL_CARDS, "capacity_slot_spell", SPELL_FACE_BOX, SPELL_MASKS, 1.0, 0),
)
GREEN = (0, 255, 0, 255)
MAX_SIZE = 1000


def scaled_box(box, size):
    left, top, right, bottom = box
    width, height = size
    return (math.floor(left * width), math.floor(top * height),
            math.ceil(right * width), math.ceil(bottom * height))


def soldier_icon_bounds(mesh_bounds):
    """保留居中卡面在 SC 坐标系中的透明留白，避免按可见网格裁紧后拉伸。"""
    left, top, right, bottom = mesh_bounds
    # 家乡兵种图标以原点为中心（普通兵半径 83，超级兵半径 150）；
    # 少数新图标使用 0..166 的坐标系，保持它们自己的画布。
    if left < 0 and top < 0:
        radius = max(abs(left), abs(top), abs(right), abs(bottom))
        return (-radius, -radius, radius, radius)
    return mesh_bounds


def compose_card(background, face, scale, face_box, masks, face_y_offset=0):
    """将 SC 卡面铺入卡底，并将动态区域及透明边角设为 green_mask。"""
    card = background.convert("RGBA").copy()
    left, top, right, bottom = scaled_box(face_box, card.size)
    face = ImageOps.fit(face.convert("RGBA"), (right - left, bottom - top),
                        method=Image.Resampling.LANCZOS)
    card.alpha_composite(face, (left, top + face_y_offset))

    size = tuple(max(1, round(value * scale)) for value in card.size)
    if max(size) > MAX_SIZE:
        raise ValueError(f"缩放后尺寸 {size[0]}×{size[1]} 超过 {MAX_SIZE} 像素上限")
    if size != card.size:
        card = card.resize(size, Image.Resampling.LANCZOS)
    # 卡底图层的不透明度低于 50% 时，才把对应位置覆盖成纯绿。
    alpha = background.getchannel("A")
    if alpha.size != size:
        alpha = alpha.resize(size, Image.Resampling.LANCZOS)
    card.paste(GREEN, mask=alpha.point(lambda value: 255 if value < 128 else 0))
    draw = ImageDraw.Draw(card)
    for box in masks:
        x0, y0, x1, y1 = scaled_box(box, size)
        draw.rectangle((x0, y0, x1 - 1, y1 - 1), fill=GREEN)
    return card.convert("RGB")


def render_cards(sc, out, scale):
    exports = dict(sc.exports())
    renderer = scrender.Renderer(sc, sctx2png.ktx_image)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".cards-", dir=out.parent) as stage_name:
        stage = Path(stage_name)
        for group, cards, background_name, face_box, masks, source_scale, face_y_offset in CARD_GROUPS:
            background_id = exports.get(background_name)
            if background_id is None:
                raise ValueError(f"{group} 缺少卡底符号 {background_name}")
            meshes = list(renderer.meshes(background_id))
            if not meshes:
                raise ValueError(f"卡底符号 {background_name} 没有可绘制网格")
            bounds = renderer.bounds(meshes, background_id)
            backgrounds = {}
            for layer in {card.background_layer for card in cards}:
                result = renderer.render(background_id, bounds=bounds, layer=layer)
                if result is None:
                    raise ValueError(f"卡底符号 {background_name} 图层 {layer} 不可绘制")
                backgrounds[layer] = result[0]
            group_stage = stage / group
            group_stage.mkdir()
            for card in cards:
                obj_id = exports.get(card.symbol)
                if obj_id is None:
                    raise ValueError(f"{card.name} 缺少卡面符号 {card.symbol}")
                face_bounds = None
                if group == "Soldier":
                    meshes = list(renderer.meshes(obj_id, layer=card.image_layer))
                    if meshes:
                        face_bounds = soldier_icon_bounds(renderer.bounds(meshes, obj_id))
                result = renderer.render(obj_id, bounds=face_bounds, layer=card.image_layer)
                if result is None:
                    raise ValueError(f"{card.name} 的卡面图层不可绘制：{card.symbol}")
                image = compose_card(backgrounds[card.background_layer], result[0],
                                     scale * source_scale, face_box, masks + card.extra_masks,
                                     face_y_offset)
                image.save(group_stage / f"{card.name}.png")
        for group, cards, *_ in CARD_GROUPS:
            target = out / group
            target.mkdir(parents=True, exist_ok=True)
            for card in cards:
                os.replace(stage / group / f"{card.name}.png",
                           target / f"{card.name}.png")
            print(f"导出 {len(cards)} 张 {group} 模板 → {target}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("source", type=Path, help="已解压的 .sc 文件")
    ap.add_argument("--out", type=Path, default=REPO / "assets/image/Cards",
                    help="合成模板的输出根目录（默认 assets/image/Cards）")
    ap.add_argument("--scale", type=float, default=1.0,
                    help="整张卡的缩放比例，默认 1")
    args = ap.parse_args(argv)
    if not math.isfinite(args.scale) or args.scale <= 0:
        ap.error("--scale 必须是大于 0 的有限数字")
    try:
        sc = scframes.ScFile(scframes.load_sc(args.source))
        render_cards(sc, args.out, args.scale)
    except (OSError, ValueError) as exc:
        ap.exit(1, f"{ap.prog}: {exc}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
