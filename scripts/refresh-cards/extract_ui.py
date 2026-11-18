#!/usr/bin/env python3
"""从资源 APK 提取家乡卡牌使用的 ui.sc 和外置图集。"""

import argparse
import re
import shutil
import struct
import tempfile
import zipfile
from pathlib import Path


UI_PATH = re.compile(rb"source_assets:/sc/(ui\.sc|ui_[0-9]+\.sctx)")


def extract(apk: Path, out: Path):
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(apk) as archive, tempfile.TemporaryDirectory(dir=out.parent) as temp:
        stage = Path(temp)
        for entry in archive.infolist():
            if not entry.filename.startswith("assets/base-attachments/") or not entry.filename.endswith(".scattachment"):
                continue
            with archive.open(entry) as source:
                header = source.read(12)
                if len(header) != 12:
                    continue
                version, offset, length = struct.unpack("<III", header)
                if version != 1 or not 12 + length <= offset < entry.file_size or length > 256:
                    continue
                name = UI_PATH.fullmatch(source.read(length).rstrip(b"\0"))
                if not name:
                    continue
                path = stage / name[1].decode("ascii")
                source.read(offset - 12 - length)
                with path.open("xb") as target:
                    shutil.copyfileobj(source, target)
        if not (stage / "ui.sc").is_file():
            raise ValueError("APK 中没有找到家乡的 ui.sc")
        out.mkdir(parents=True, exist_ok=True)
        for path in sorted(stage.iterdir()):
            path.replace(out / path.name)
            print(out / path.name)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("apk", type=Path, help="已拉取的资源 APK")
    cli.add_argument("out", type=Path, help="保存 ui.sc 和图集的目录")
    args = cli.parse_args()
    try:
        extract(args.apk, args.out)
    except (OSError, ValueError, zipfile.BadZipFile) as exc:
        cli.exit(1, f"{cli.prog}: {exc}\n")


if __name__ == "__main__":
    main()
