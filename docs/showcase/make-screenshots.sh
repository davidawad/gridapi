#!/usr/bin/env bash
# Regenerates every checked-in showcase image. Offline: the examples serve synthetic NYISO files
# (the real layout, invented prices) from a local mock server and decode them with gridapi-nyiso.
# Needs cargo and rsvg-convert (brew install librsvg). Usage: docs/showcase/make-screenshots.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
out=docs/showcase

# 1. Hero chart: day-ahead LBMP by zone.
cargo run --quiet -p gridapi-nyiso --example showcase_chart > "$out/nyiso-zones.svg"
rsvg-convert -w 1200 "$out/nyiso-zones.svg" -o "$out/nyiso-zones.png"

# 2. Terminal render: the CLI-style table, as text and as an image.
cargo run --quiet -p gridapi-nyiso --example showcase > "$out/nyiso-day-ahead.txt"
{
  lines=$(wc -l < "$out/nyiso-day-ahead.txt")
  height=$(( lines * 26 + 76 ))
  printf '<svg xmlns="http://www.w3.org/2000/svg" width="900" height="%s" font-family="Menlo, Monaco, Courier New, monospace" font-size="18">\n' "$height"
  printf '<rect width="900" height="%s" rx="12" fill="#1e2127"/>\n' "$height"
  printf '<circle cx="28" cy="26" r="7" fill="#ff5f56"/><circle cx="52" cy="26" r="7" fill="#ffbd2e"/><circle cx="76" cy="26" r="7" fill="#27c93f"/>\n'
  printf '<text x="28" y="66" fill="#7f848e" xml:space="preserve">$ cargo run -p gridapi-nyiso --example showcase</text>\n'
  y=66
  while IFS= read -r line; do
    y=$(( y + 26 ))
    esc=${line//&/&amp;}; esc=${esc//</&lt;}; esc=${esc//>/&gt;}
    printf '<text x="28" y="%s" fill="#d7dae0" xml:space="preserve">%s</text>\n' "$y" "$esc"
  done < "$out/nyiso-day-ahead.txt"
  printf '</svg>\n'
} > "$out/nyiso-day-ahead-terminal.svg"
rsvg-convert -w 900 "$out/nyiso-day-ahead-terminal.svg" -o "$out/nyiso-day-ahead-terminal.png"

# Every image stays under 300 KB.
for f in "$out"/*.png; do
  size=$(wc -c < "$f")
  [ "$size" -lt 300000 ] || { echo "$f is $size bytes (limit 300000)" >&2; exit 1; }
  echo "$f $size bytes"
done
