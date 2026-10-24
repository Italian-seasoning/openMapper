#!/bin/sh
# Renders the pictures of the Windows installer (installer-sidebar.bmp, installer-header.bmp) and of
# the macOS disk image (dmg-background.png, @2x) from the SVGs in assets/icons/app, in Inter. Needs
# resvg (cargo install resvg) and Python 3 with Pillow.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/../assets/icons/app" && pwd)"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
resvg -w 328 installer-sidebar.svg "$tmp/sidebar.png"
resvg -w 300 installer-header.svg "$tmp/header.png"
resvg --use-font-file ../../fonts/InterVariable.ttf -w 540 dmg-background.svg dmg-background.png
resvg --use-font-file ../../fonts/InterVariable.ttf -w 1080 dmg-background.svg dmg-background@2x.png
python3 - "$tmp" <<'PY'
import sys
from PIL import Image
t = sys.argv[1]
for n in ("sidebar", "header"):
    Image.open(f"{t}/{n}.png").convert("RGB").quantize(256).save(f"installer-{n}.bmp")
PY
echo "installer pictures written to assets/icons/app"
