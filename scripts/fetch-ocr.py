#!/usr/bin/env python3
"""Fetch the pinned MaaCommonAssets PP-OCR v4 Chinese model (Apache-2.0)."""
import hashlib
from pathlib import Path
import urllib.request

REVISION = "dabcd4681ac990dc4361de26416d986abd80e4aa"
FILES = {
    "det.onnx": "767e663dded50813dca0abc9a49859e345e0f74a48b387661188f096a9211d74",
    "rec.onnx": "da4815ea386ee8083be319da8d20e70f4fdb0b6404ba0d2da7f675831e1b37fa",
    "keys.txt": "28b2362ad4ab2dc38769aa72feb535e3a9ddb3fd2a7585a05920e6393b1dc7f7",
}


def main():
    root = Path(__file__).resolve().parent.parent / "assets/model/ocr"
    root.mkdir(parents=True, exist_ok=True)
    for name, digest in FILES.items():
        path = root / name
        if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest() == digest:
            print(f"OCR 已就绪: {name}")
            continue
        url = f"https://raw.githubusercontent.com/MaaXYZ/MaaCommonAssets/{REVISION}/OCR/ppocr_v4/zh_cn/{name}"
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != digest:
            raise RuntimeError(f"OCR 校验失败: {name}")
        temporary = path.with_suffix(path.suffix + ".part")
        temporary.write_bytes(data)
        temporary.replace(path)
        print(f"OCR 下载完成: {name}")


if __name__ == "__main__":
    main()
