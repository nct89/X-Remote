#!/usr/bin/env python3
"""Scale `x-remote-logo.png` as-is. Do not cut or recolor."""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent
SOURCE = REPO / "x-remote-logo.png"
CLIENT_ASSETS = REPO / "crates" / "client" / "assets"


def write_rgba(im: Image.Image, path: Path) -> None:
    path.write_bytes(im.convert("RGBA").tobytes())


def main() -> None:
    if not SOURCE.exists():
        raise SystemExit(f"missing {SOURCE}")
    src = Image.open(SOURCE).convert("RGBA")
    CLIENT_ASSETS.mkdir(parents=True, exist_ok=True)
    outputs = {
        1024: ROOT / "icon-1024.png",
        256: ROOT / "icon.png",
        128: ROOT / "x-remote.png",
        64: ROOT / "icon-64.png",
        48: ROOT / "icon-48.png",
        32: ROOT / "icon-32.png",
    }
    for size, dest in outputs.items():
        src.resize((size, size), Image.Resampling.LANCZOS).save(dest)
        print(f"wrote {dest.name}")
    write_rgba(src.resize((128, 128), Image.Resampling.LANCZOS), CLIENT_ASSETS / "icon-128.rgba")
    write_rgba(src.resize((256, 256), Image.Resampling.LANCZOS), CLIENT_ASSETS / "icon-256.rgba")


if __name__ == "__main__":
    main()
