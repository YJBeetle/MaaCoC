"""Extract Supercell `*.sctx` textures from a CoC APK into PNGs.

CoC's SCTX layout differs from the Clash Royale one the public dumpers assume:

    0   u32     header length (48 / 52 / 56, or thousands with a record table)
    8   b"SCTX"
    12  u16 u16 ASTC block size hint, unreliable across variants
    ?   u16 u16 width, height -- at `header - 8` for zstd textures, again just
                  after `header` for the uncompressed atlases
    ..  zstd block holding raw ASTC, or raw ASTC straight after the header

Neither the block size nor the payload offset is trustworthy, so both are
recovered by the geometry identity: a candidate is accepted only when
ceil(w/b) * ceil(h/b) * 16 equals the payload length exactly, which is what
keeps a misread header from producing a garbage image.

The `*.sc` sprite collections are read too: each is a name table plus a zstd
body wrapping whole KTX atlases, and those pages hold the unit info renders
that the in-game card face is cropped from.

Writes `<dest>/index.html` at the end: every image, grouped by folder, on a
checkerboard so transparency is visible.
"""

import argparse
import struct
import sys
import urllib.parse
import zipfile
from html import escape
from math import ceil
from pathlib import Path

import texture2ddecoder
import zstandard
from PIL import Image

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DEST = REPO_ROOT / "var" / "coc-unpack" / "png"

BLOCKS = (4, 5, 6, 8, 10, 12)
ZSTD = b"\x28\xb5\x2f\xfd"
SCLZ = b"SCLZ"
KTX = bytes([0xAB]) + b"KTX 11" + bytes([0xBB, 0x0D, 0x0A, 0x1A, 0x0A])
# KTX 1.1 names the ASTC block shape as a GL enum, 0x93B0 upwards.
ASTC_BLOCKS = ((4, 4), (5, 4), (5, 5), (6, 5), (6, 6), (8, 5), (8, 6), (8, 8),
               (10, 5), (10, 6), (10, 8), (10, 10), (12, 10), (12, 12))

GALLERY_HEAD = """<!doctype html>
<meta charset="utf-8">
<title>CoC 解包素材</title>
<style>
body { margin: 24px; background: #17181a; color: #e8eaed; font: 13px/1.5 system-ui, sans-serif; }
h2 { margin: 28px 0 10px; font-size: 15px; font-weight: 500; border-bottom: 1px solid #2c2f33; padding-bottom: 6px; }
h2 small { opacity: .5; font-weight: 400; }
.grid { display: flex; flex-wrap: wrap; gap: 14px; align-items: flex-start; }
figure { margin: 0; width: 190px; text-align: center; }
img { max-width: 180px; max-height: 150px; display: block; margin: 0 auto;
      background: repeating-conic-gradient(#3a3d42 0 25%, #2a2c30 0 50%) 0 0 / 16px 16px; }
figcaption { margin-top: 4px; font-size: 11px; opacity: .65; word-break: break-all; }
nav { position: sticky; top: 0; background: #17181a; padding: 10px 0; font-size: 12px; line-height: 2;
      border-bottom: 1px solid #2c2f33; max-height: 40vh; overflow: auto; }
nav a { color: #8ab4f8; text-decoration: none; margin-right: 4px; }
nav b { color: #e8eaed; font-weight: 500; }
</style>
"""


def dimension_offsets(header: int, size: int) -> list[int]:
    """Where width/height may sit, best guess first.

    Short files store them at `header - 8`; the uncompressed atlases repeat them
    just *after* the header instead, so the scan runs past `header` as well.
    """
    limit = min(header + 256, size - 4)
    candidates = [header - 8, header + 32, 40, 44, 48, 52]
    candidates += [offset for offset in range(32, limit, 2) if offset not in candidates]
    return [offset for offset in candidates if 0 < offset <= limit]


def mip_total(width: int, height: int, block: int) -> int:
    """Full mip-chain byte count for one ASTC block shape."""
    total = 0
    level_w, level_h = width, height
    while True:
        total += ceil(level_w / block) * ceil(level_h / block) * 16
        if level_w == 1 and level_h == 1:
            return total
        level_w, level_h = max(1, level_w // 2), max(1, level_h // 2)


def decode(data: bytes) -> Image.Image | None:
    if data[8:12] != b"SCTX":
        return None
    header = struct.unpack_from("<I", data, 0)[0]
    compressed = data.find(ZSTD, 32)
    if compressed < 0 and data.find(SCLZ, 32) >= 0:
        raise NotImplementedError("LZHAM payload needs pylzham, which will not build here")
    pixels = zstandard.decompress(data[compressed:]) if compressed >= 0 else None
    for offset in dimension_offsets(header, len(data)):
        width, height = struct.unpack_from("<HH", data, offset)
        if not (16 <= width <= 8192 and 16 <= height <= 8192):
            continue
        for block in BLOCKS:
            need = ceil(width / block) * ceil(height / block) * 16
            if pixels is not None:
                if need != len(pixels):
                    # The sc3d model textures store a full mip chain, so only
                    # the first level is `need` bytes and the rest follows.
                    if mip_total(width, height, block) != len(pixels):
                        continue
                    blob = pixels[:need]
                else:
                    blob = pixels
            else:
                # Uncompressed atlases follow their record table with raw ASTC,
                # so the payload start is implied by its length.
                start = len(data) - need
                if not header <= start <= header + 256:
                    continue
                blob = data[start:]
            rgba = texture2ddecoder.decode_astc(blob, width, height, block, block)
            return Image.frombytes("RGBA", (width, height), rgba, "raw", "BGRA")
    return None


def asset_parts(name: str) -> tuple[str, ...]:
    """Path relative to the asset root.

    An APK stores things under `assets/`, a pulled content directory under
    `<pkg>/update/`; both describe the same layout, so normalising here lets
    one categorisation rule serve both.
    """
    parts = Path(name).parts
    for marker in ("assets", "update"):
        if marker in parts:
            return parts[parts.index(marker) + 1 :]
    return parts


def out_rel(name: str) -> Path:
    """Categorised destination, keyed on where the texture sits.

    The game groups sprites by use, so reusing that grouping keeps a full
    extraction browsable instead of one flat dump of several thousand files.
    """
    parts = asset_parts(name)
    if not parts:
        return Path("misc", Path(name).with_suffix(".png").name)
    stem = Path(name).stem
    group = stem.split("_")[0]
    tail = Path(*parts)
    if parts[0] == "sc3d":
        return Path("sc3d", *parts[1:]).with_suffix(".png")
    if parts[:2] == ("ui", "sc"):
        return Path("ui", group, *parts[2:]).with_suffix(".png")
    if parts[0] == "image":
        if len(parts) == 2:
            return Path("image", "misc", *parts[1:]).with_suffix(".png")
        return Path("image", *parts[1:]).with_suffix(".png")
    if parts[0] == "sc":
        return Path("sc", group, *parts[1:]).with_suffix(".png")
    return Path("misc", tail).with_suffix(".png")


def iter_files(source: Path, suffixes: tuple[str, ...]):
    """Yield (name, bytes) for an APK or for every matching file under a directory.

    A pulled content tree carries gigabytes of `.glb` and audio that this tool
    cannot decode, so filter before reading rather than after.
    """
    if source.is_dir():
        for path in sorted(source.rglob("*")):
            if path.is_file() and path.suffix in suffixes:
                yield path.relative_to(source).as_posix(), path.read_bytes()
        return
    with zipfile.ZipFile(source) as zf:
        for name in zf.namelist():
            if name.endswith(suffixes):
                yield name, zf.read(name)


def ktx_pages(blob: bytes):
    """Every KTX page inside a decompressed `.sc` body, as (name-less) images."""
    pos = blob.find(KTX)
    while pos >= 0:
        _, _, _, _, internal, _, width, height, _, _, _, _, kv = struct.unpack_from("<13I", blob, pos + 12)
        index = internal - 0x93B0
        if 0 <= index < len(ASTC_BLOCKS):
            block = ASTC_BLOCKS[index]
            size = struct.unpack_from("<I", blob, pos + 64 + kv)[0]
            data = blob[pos + 68 + kv : pos + 68 + kv + size]
            if ceil(width / block[0]) * ceil(height / block[1]) * 16 == size:
                rgba = texture2ddecoder.decode_astc(data, width, height, *block)
                yield Image.frombytes("RGBA", (width, height), rgba, "raw", "BGRA")
        pos = blob.find(KTX, pos + 64)


def save(image: Image.Image, target: Path, max_edge: int) -> None:
    if max_edge and max(image.size) > max_edge:
        ratio = max_edge / max(image.size)
        image = image.resize((max(1, round(image.width * ratio)), max(1, round(image.height * ratio))), Image.LANCZOS)
    target.parent.mkdir(parents=True, exist_ok=True)
    image.save(target)


def extract(source: Path, out: Path, force: bool, contains: str, max_edge: int) -> tuple[int, int, list[str]]:
    done, skipped, failed = 0, 0, []
    for name, data in iter_files(source, (".sctx",)):
        if contains and contains not in name:
            continue
        target = out / out_rel(name)
        if target.exists() and not force:
            skipped += 1
            continue
        try:
            image = decode(data)
        except Exception as exc:
            failed.append(f"{name}: {type(exc).__name__}")
            continue
        if image is None:
            failed.append(name)
            continue
        save(image, target, max_edge)
        done += 1
        if (done + skipped) % 200 == 0:
            print(f"  ... {done + skipped}", flush=True)
    return done, skipped, failed


def extract_sc(source: Path, out: Path, force: bool, contains: str, max_edge: int) -> tuple[int, int, int]:
    """Sprite collections ship their own texture pages.

    A `.sc` is a name table plus a zstd body, and the body holds whole KTX
    atlases -- this is where the unit info renders and the card faces live.
    """
    done = skipped = failed = 0
    for name, raw in iter_files(source, (".sc",)):
        if contains and contains not in name:
            continue
        start = raw.find(ZSTD)
        stem = Path(name).stem
        if start < 0:
            failed += 1
            continue
        try:
            pages = list(ktx_pages(zstandard.decompress(raw[start:])))
        except Exception:
            failed += 1
            continue
        for index, image in enumerate(pages):
            target = out / "sc-ktx" / f"{stem}_{index}_{image.width}x{image.height}.png"
            if target.exists() and not force:
                skipped += 1
                continue
            save(image, target, max_edge)
            done += 1
    return done, skipped, failed


def write_gallery(out: Path) -> Path:
    """One HTML page that shows everything extracted, grouped by folder.

    Several hundred loose PNGs are not browsable in Finder; a lazy-loading
    contact sheet on a checkerboard is, and it shows through the transparency
    that decides whether a sprite is usable.
    """
    groups: dict[str, list[Path]] = {}
    for path in sorted(out.rglob("*.png")):
        groups.setdefault(path.parent.relative_to(out).as_posix(), []).append(path)

    parts = [GALLERY_HEAD]
    parts.append('<nav>' + ' · '.join(
        f'<a href="#{escape(g)}">{escape(g)}&nbsp;<b>{len(p)}</b></a>'
        for g, p in sorted(groups.items())) + '</nav>')
    for group, paths in groups.items():
        parts.append(f'<h2 id="{escape(group)}">{escape(group)} <small>{len(paths)}</small></h2><div class="grid">')
        for path in paths:
            rel = path.relative_to(out).as_posix()
            with Image.open(path) as image:
                width, height = image.size
            href = escape(urllib.parse.quote(rel))
            parts.append(
                f'<figure><a href="{href}"><img loading="lazy" src="{href}" '
                f'width="{width}" height="{height}" alt=""></a>'
                f"<figcaption>{escape(path.name)}<br>{width}×{height}</figcaption></figure>"
            )
        parts.append("</div>")
    parts.append("</body></html>")
    page = out / "index.html"
    page.write_text("\n".join(parts), encoding="utf-8")
    return page


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", nargs="*", type=Path,
                        help="APK files or directories to read (default: every apk/*.apk)")
    parser.add_argument("-d", "--dest", type=Path, default=DEFAULT_DEST)
    parser.add_argument("-c", "--contains", default="", help="only extract paths containing this text")
    parser.add_argument("-f", "--force", action="store_true", help="re-decode files that already exist")
    parser.add_argument("--max-edge", type=int, default=0,
                        help="downscale saved pages so the longest edge fits, keeping a huge dump browsable")
    args = parser.parse_args()

    sources = args.source or sorted((args.dest.parent / "apk").glob("*.apk"))
    if not sources:
        print("没有可解包的来源", file=sys.stderr)
        return 1

    total_done = total_skipped = total_sc_failed = 0
    all_failed: list[str] = []
    for source in sources:
        print(f"解包 {source}", flush=True)
        done, skipped, failed = extract(source, args.dest, args.force, args.contains, args.max_edge)
        print(f"  sctx  新写 {done}，已存在 {skipped}，未解析 {len(failed)}", flush=True)
        total_done += done
        total_skipped += skipped
        all_failed += failed
        done, skipped, failed = extract_sc(source, args.dest, args.force, args.contains, args.max_edge)
        print(f"  sc    新写 {done}，已存在 {skipped}，无内嵌贴图 {failed}", flush=True)
        total_done += done
        total_skipped += skipped
        total_sc_failed += failed

    print(f"\n合计：新写 {total_done}，已存在 {total_skipped}，sctx 未解析 {len(all_failed)}，sc 无贴图 {total_sc_failed}")
    for item in all_failed[:20]:
        print("  -", item)
    print(f"\n浏览： {write_gallery(args.dest)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
