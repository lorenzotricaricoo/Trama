#!/usr/bin/env python3
"""Renders the app icon from `src-tauri/icons/source/ordito.svg`.

- The bundle icons (`icons/icon.icns`, the PNGs and `icon.ico`) in Lime, through
  `tauri icon`: Finder, Launchpad, Spotlight and the Dock while Trama is closed.
- One Dock icon per accent (`icons/dock/<accent>.png`), which the app shows while it is
  open (see `src-tauri/src/dock_icon.rs`).
- The same icons with a "DEV" badge for debug builds: the bundle icons in `icons/debug/`
  (used by `scripts/bundle-macos.sh --debug` through `tauri.debug.conf.json`) and the Dock
  icons in `icons/debug/dock/`, Lime included.

The accents differ only in hue (`.acc-<name>` in docs/design/tokens.css): every lime of
the source is turned on the accent's hue, keeping its lightness, and graphite also takes
its low chroma. Needs `rsvg-convert` (`brew install librsvg`). Run from `apps/desktop`:

    python3 scripts/app-icon.py
"""

import math
import pathlib
import re
import shutil
import subprocess
import tempfile

ICONS = pathlib.Path("src-tauri/icons")
SOURCE = ICONS / "source" / "ordito.svg"

# Hue and chroma of each accent, as in docs/design/tokens.css.
BASE_CHROMA = 0.17
ACCENTS = {
    "lime": (128, BASE_CHROMA),
    "blue": (250, BASE_CHROMA),
    "purple": (300, BASE_CHROMA),
    "pink": (350, BASE_CHROMA),
    "red": (25, BASE_CHROMA),
    "orange": (55, BASE_CHROMA),
    "yellow": (90, BASE_CHROMA),
    "green": (155, BASE_CHROMA),
    "graphite": (270, 0.012),
}

# The limes of the source: the warp threads (light to full) and the glow.
LIMES = ["#bbea7a", "#85b72e", "#5d7e28"]

# The Dock draws icons up to 256 pt, 512 px on a Retina display.
DOCK_SIZE = 512

# Files of the bundle (`bundle.icon` in tauri.conf.json).
BUNDLE_FILES = ["32x32.png", "128x128.png", "128x128@2x.png", "icon.icns", "icon.ico", "icon.png"]

# Files of the debug bundle (`bundle.icon` in tauri.debug.conf.json): macOS only.
DEBUG_FILES = ["32x32.png", "128x128.png", "128x128@2x.png", "icon.icns"]

# "DEV" in a light pill across the bottom threads, outlined in the frame's graphite so it
# reads on every accent and at 32 px. Neutral on purpose: red stays for recording, and the
# accent tint (which only swaps the limes) leaves it alone. The letters are paths, so the
# render does not depend on the fonts installed.
BADGE = """
  <g filter="url(#shadow)">
    <rect x="292" y="736" width="440" height="160" rx="80" fill="#f4f4f6"
          stroke="#0d0f15" stroke-width="14"/>
    <g transform="translate(512 816) scale(1.05) translate(-118 -42)" fill="#0d0f15">
      <path fill-rule="evenodd" d="M0 0 H32 A42 42 0 0 1 32 84 H0 Z M21 21 H32 A21 21 0 0 1 32 63 H21 Z"/>
      <path d="M88 0 H148 V20 H109 V32 H143 V52 H109 V64 H148 V84 H88 Z"/>
      <path d="M162 0 H184 L199 56 L214 0 H236 L210 84 H188 Z"/>
    </g>
  </g>
"""


def to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def to_gamma(c):
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def hex_to_oklch(value):
    r, g, b = (to_linear(int(value[i : i + 2], 16) / 255) for i in (1, 3, 5))
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    lab_l = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s
    lab_a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s
    lab_b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s
    return lab_l, math.hypot(lab_a, lab_b), math.degrees(math.atan2(lab_b, lab_a)) % 360


def oklch_to_rgb(lightness, chroma, hue):
    a = chroma * math.cos(math.radians(hue))
    b = chroma * math.sin(math.radians(hue))
    l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (lightness - 0.0894841775 * a - 1.2914855480 * b) ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def oklch_to_hex(lightness, chroma, hue):
    """The colour in sRGB, with the chroma lowered until it fits."""
    low, high = 0.0, chroma
    rgb = oklch_to_rgb(lightness, 0, hue)
    for _ in range(30):
        mid = (low + high) / 2
        candidate = oklch_to_rgb(lightness, mid, hue)
        if all(-1e-6 <= c <= 1 + 1e-6 for c in candidate):
            low, rgb = mid, candidate
        else:
            high = mid
    if all(-1e-6 <= c <= 1 + 1e-6 for c in oklch_to_rgb(lightness, chroma, hue)):
        rgb = oklch_to_rgb(lightness, chroma, hue)
    return "#" + "".join(f"{round(to_gamma(min(max(c, 0), 1)) * 255):02x}" for c in rgb)


def tinted(svg, hue, chroma):
    for lime in LIMES:
        lightness, lime_chroma, _ = hex_to_oklch(lime)
        color = oklch_to_hex(lightness, lime_chroma * chroma / BASE_CHROMA, hue)
        svg = re.sub(re.escape(lime), color, svg, flags=re.IGNORECASE)
    return svg


def render(svg_path, png_path, size):
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(png_path), str(svg_path)],
        check=True,
    )


def badged(svg):
    return svg.replace("</svg>", BADGE + "</svg>")


def render_set(source, scratch, dock_dir, bundle_dir, bundle_files):
    """The Dock icon of every accent and the bundle icons, from one source."""
    dock_dir.mkdir(parents=True, exist_ok=True)
    bundle_dir.mkdir(parents=True, exist_ok=True)
    for name, (hue, chroma) in ACCENTS.items():
        svg = scratch / f"{name}.svg"
        svg.write_text(tinted(source, hue, chroma))
        render(svg, dock_dir / f"{name}.png", DOCK_SIZE)

    svg = scratch / "icon.svg"
    svg.write_text(source)
    master = scratch / "icon-1024.png"
    render(svg, master, 1024)
    out = scratch / "bundle"
    subprocess.run(["npx", "tauri", "icon", str(master), "-o", str(out)], check=True)
    for name in bundle_files:
        shutil.copyfile(out / name, bundle_dir / name)


def main():
    source = SOURCE.read_text()
    for kind, svg, dock_dir, bundle_dir, files in [
        ("release", source, ICONS / "dock", ICONS, BUNDLE_FILES),
        ("debug", badged(source), ICONS / "debug" / "dock", ICONS / "debug", DEBUG_FILES),
    ]:
        with tempfile.TemporaryDirectory() as scratch:
            render_set(svg, pathlib.Path(scratch), dock_dir, bundle_dir, files)
        print(f"Rendered the {kind} bundle icons and {len(ACCENTS)} Dock icons.")


if __name__ == "__main__":
    main()
