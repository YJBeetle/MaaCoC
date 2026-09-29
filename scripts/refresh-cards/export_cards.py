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

from PIL import Image, ImageDraw

# 同级拿 scframes，上一级 scripts/ 拿共用的 KTX 解码 sctx2png
HERE = Path(__file__).resolve().parent
sys.path[:0] = [str(HERE), str(HERE.parent)]
import scframes          # noqa: E402
import scrender          # noqa: E402
import sctx2png          # noqa: E402

REPO = HERE.parents[1]

# 卡牌名、SC 符号、可选的卡面图层，以及特殊卡底。
# Soldier 沿用原模板的编号前缀，方便按兵种顺序浏览。
class CardSpec(NamedTuple):
    name: str
    symbol: str
    image_layer: int | None = None
    background_layer: int | None = None
    background_symbol: str | None = None
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
    CardSpec("100_Broom", "icon_unit_majo", background_layer=2),
    CardSpec("101_BarbarianKicker", "icon_unit_footballbarbarian", background_layer=2),
    CardSpec("102_M.E.C.H.A", "icon_unit_mecha", background_layer=2),
    CardSpec("103_BattleRam", "icon_unit_battleram_cc", background_layer=2),
    CardSpec("104_IceWizard", "icon_unit_icewizard", background_layer=2),
    CardSpec("105_DebtCollector", "icon_unit_goblin_tax_collector", background_layer=2),
    CardSpec("106_GiantThrower", "icon_unit_footballgiant", background_layer=2),
    CardSpec("107_Firecracker", "icon_unit_firecracker", background_layer=2),
    CardSpec("108_RamRider", "icon_unit_cookie_ramrider", background_layer=2),
    CardSpec("109_PartyWizard", "icon_unit_partyWizard", background_layer=2),
    CardSpec("110_MeteorGolem", "icon_unit_splitgolem", background_layer=2),
    CardSpec("111_IceMinion", "icon_unit_ice_minion", background_layer=2),
    CardSpec("112_Lavaloon", "icon_unit_lavaloon", background_layer=2),
    CardSpec("113_Barcher", "icon_unit_barcher", background_layer=2),
    CardSpec("SuperBalloon", "icon_unit_elite_balloon", background_layer=1,
             background_symbol="capacity_slot"),
    CardSpec("SuperDragon", "icon_unit_elite_dragon", background_layer=1,
             background_symbol="capacity_slot"),
    CardSpec("SuperMiner", "icon_unit_elite_miner", background_layer=1,
             background_symbol="capacity_slot"),
    CardSpec("SuperWitch", "icon_unit_elite_witch", background_layer=1,
             background_symbol="capacity_slot"),
)

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
# 卡面在各自卡底的原始 SC 画布中的位置。普通卡面约为 83×83，英雄为 83×113。
SOLDIER_FACE_BOX = (3.5, 24.4, 78.5, 99)
HERO_FACE_BOX = (2.5, -4, 76.5, 98)
SPELL_FACE_BOX = (2.5, 24.4, 77, 99)
SUPER_FACE_BOX = (3, 22, 68, 91)
# 英雄肖像的可见网格略有差异，但共享同一个 166×225 的 SC 画布。
HERO_ICON_BOUNDS = (-83, -144.5, 83, 80.5)
# 数量、等级等动态内容由 MAAFW 的 green_mask 跳过。
SOLDIER_MASKS = ((0.39, 0.03, 0.95, 0.2), (0.06, 0.68, 0.4, 0.91))
HERO_MASKS = ((0.03, 0.06, 0.43, 0.36), (0.05, 0.68, 0.38, 0.93))
SPELL_MASKS = ((0.39, 0.03, 0.95, 0.21), (0.05, 0.68, 0.38, 0.91))
CARD_GROUPS = (
    ("Soldier", SOLDIER_CARDS, "unit_slot", 1, SOLDIER_FACE_BOX, SOLDIER_MASKS),
    ("Hero", HERO_CARDS, "hero_slot", 2, HERO_FACE_BOX, HERO_MASKS),
    ("Spell", SPELL_CARDS, "spell_slot", 1, SPELL_FACE_BOX, SPELL_MASKS),
)
GREEN = (0, 255, 0, 255)
MAX_SIZE = 1000
# 根据设备截图缩至 720 高后的实测卡牌尺寸区分卡底倍率。
SOLDIER_CARD_SCALE = 1.69 * 720 / 1080
HERO_SPELL_CARD_SCALE = 1.69 * 720 / 1080
# 旧的 capacity_slot or attack_confirm_troop 约 71×94，超级兵暂沿用它的红底并校正到相近的输出尺寸。
SUPER_CARD_SCALE = 1.25


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


def fit_face_meshes(meshes, face_bounds, canvas_bounds, face_box):
    """按同类卡牌的固定位置把顶点映射到卡底画布。"""
    left, top, right, bottom = face_box
    min_x, min_y, max_x, max_y = face_bounds
    face_width = math.ceil(max_x - min_x)
    face_height = math.ceil(max_y - min_y)
    factor = max((right - left) / face_width, (bottom - top) / face_height)
    source_center_x = min_x + face_width / 2
    source_center_y = min_y + face_height / 2
    target_center_x = canvas_bounds[0] + (left + right) / 2
    target_center_y = canvas_bounds[1] + (top + bottom) / 2
    fitted = [(page, [(target_center_x + (x - source_center_x) * factor,
                       target_center_y + (y - source_center_y) * factor, u, v)
                      for x, y, u, v in points])
              for page, points in meshes]
    clip_box = (canvas_bounds[0] + left, canvas_bounds[1] + top,
                canvas_bounds[0] + right, canvas_bounds[1] + bottom)
    return fitted, clip_box


def compose_card(renderer, background_meshes, background_bounds, face_meshes,
                 face_bounds, scale, face_box, masks):
    """卡底、卡面网格在同一画布各绘制一次，再覆盖动态区域。"""
    natural = (math.ceil(background_bounds[2] - background_bounds[0]),
               math.ceil(background_bounds[3] - background_bounds[1]))
    size = tuple(max(1, round(value * scale)) for value in natural)
    if max(size) > MAX_SIZE:
        raise ValueError(f"缩放后尺寸 {size[0]}×{size[1]} 超过 {MAX_SIZE} 像素上限")
    canvas_bounds = (background_bounds[0], background_bounds[1],
                     background_bounds[0] + natural[0],
                     background_bounds[1] + natural[1])
    card = Image.new("RGBA", size)
    renderer.draw_meshes(card, background_meshes, canvas_bounds)
    # 卡底图层的不透明度低于 50% 时，才把对应位置覆盖成纯绿。
    alpha = card.getchannel("A")
    fitted, clip_box = fit_face_meshes(face_meshes, face_bounds, canvas_bounds,
                                       face_box)
    renderer.draw_meshes(card, fitted, canvas_bounds, clip_box=clip_box)
    card.paste(GREEN, mask=alpha.point(lambda value: 255 if value < 128 else 0)) # 绿色蒙版 Alpha 阈值
    draw = ImageDraw.Draw(card)
    for box in masks:
        x0, y0, x1, y1 = scaled_box(box, size)
        draw.rectangle((x0, y0, x1 - 1, y1 - 1), fill=GREEN)
    return card.convert("RGB")


def render_cards(sc, out):
    exports = dict(sc.exports())
    renderer = scrender.Renderer(sc, sctx2png.ktx_image)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".cards-", dir=out.parent) as stage_name:
        stage = Path(stage_name)
        backgrounds = {}
        for group, cards, background_name, default_layer, face_box, masks in CARD_GROUPS:
            group_stage = stage / group
            group_stage.mkdir()
            for card in cards:
                selected_background = card.background_symbol or background_name
                selected_layer = (card.background_layer if card.background_layer is not None
                                  else default_layer)
                key = (selected_background, selected_layer)
                if key not in backgrounds:
                    background_id = exports.get(selected_background)
                    if background_id is None:
                        raise ValueError(f"{group} 缺少卡底符号 {selected_background}")
                    layer_meshes = list(renderer.meshes(background_id,
                                                        layer=selected_layer))
                    if not layer_meshes:
                        raise ValueError(f"卡底符号 {selected_background} 图层 {selected_layer} 不可绘制")
                    # hero_slot 顶部还有独立装饰，卡牌画布以指定卡底图层为准。
                    bounds = renderer.bounds(layer_meshes, background_id)
                    if selected_background == "unit_slot":
                        whole = list(renderer.meshes(background_id))
                        bounds = renderer.bounds(whole, background_id)
                    backgrounds[key] = layer_meshes, bounds
                obj_id = exports.get(card.symbol)
                if obj_id is None:
                    raise ValueError(f"{card.name} 缺少卡面符号 {card.symbol}")
                face_meshes = list(renderer.meshes(obj_id, layer=card.image_layer))
                if not face_meshes:
                    raise ValueError(f"{card.name} 的卡面图层不可绘制：{card.symbol}")
                face_bounds = renderer.bounds(face_meshes, obj_id)
                if group == "Soldier":
                    face_bounds = soldier_icon_bounds(face_bounds)
                elif group == "Hero":
                    face_bounds = HERO_ICON_BOUNDS
                layer_meshes, bounds = backgrounds[key]
                is_super = selected_background == "capacity_slot"
                scale = (SUPER_CARD_SCALE if is_super else
                         SOLDIER_CARD_SCALE if group == "Soldier" else
                         HERO_SPELL_CARD_SCALE)
                image = compose_card(renderer, layer_meshes, bounds, face_meshes,
                                     face_bounds, scale,
                                     SUPER_FACE_BOX if is_super else face_box,
                                     masks + card.extra_masks)
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
    args = ap.parse_args(argv)
    try:
        sc = scframes.ScFile(scframes.load_sc(args.source))
        render_cards(sc, args.out)
    except (OSError, ValueError) as exc:
        ap.exit(1, f"{ap.prog}: {exc}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
