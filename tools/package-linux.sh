#!/bin/sh
# Builds openMapper and a tar.gz of it in target/package (Linux): the program, a desktop
# entry to put in ~/.local/share/applications, the README and the tutorial (in docs, where
# the README links it).
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(uname -m)
cargo build --release --locked
OUT=target/package
NAME="openMapper-$VERSION-linux-$ARCH"
DIR="$OUT/$NAME"
rm -rf "$DIR"
mkdir -p "$DIR/docs"
cp target/release/openmapper "$DIR/openmapper"
strip "$DIR/openmapper" 2>/dev/null || true
cp README.md "$DIR/"
cp docs/TUTORIAL.md "$DIR/docs/"
[ -f LICENSE ] && cp LICENSE "$DIR/"
cat > "$DIR/openmapper.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=openMapper
Comment=A simple map editor for openOMSI
Exec=openmapper %f
Terminal=false
Categories=Game;Simulation;
DESKTOP
(cd "$OUT" && rm -f "$NAME.tar.gz" && tar -czf "$NAME.tar.gz" "$NAME")
echo "$OUT/$NAME.tar.gz"
