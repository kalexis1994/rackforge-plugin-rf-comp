#!/usr/bin/env python3
"""Generate the original RF-Comp branding assets RackForge requires.

RackForge validates three PNGs by exact size: a 512x512 icon, a 1600x400
banner and a 1920x1080 splash. They are drawn here rather than committed as
opaque binaries so the visual identity is reviewable, reproducible, and
unmistakably this project's own work: a transfer curve bending at its
threshold over a soft knee, and the unity line it leaves.

Run:  python tools/generate-branding.py
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "plugin" / "package" / "branding"

FONT_CANDIDATES = (
    Path("C:/Windows/Fonts/arialbd.ttf"),
    Path("C:/Windows/Fonts/segoeuib.ttf"),
    Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
)

# The palette also lives in rackforge-plugin.toml and web/play.html; keep the
# three in step.
PANEL = (14, 18, 22)
PANEL_LIGHT = (27, 35, 43)
INK = (232, 236, 239)
MUTED = (139, 150, 160)
ACCENT = (232, 163, 61)
ACCENT_DIM = (110, 74, 22)
STEEL = (174, 182, 189)
GRID = (40, 48, 56)


def font(size: int) -> ImageFont.FreeTypeFont:
    for candidate in FONT_CANDIDATES:
        if candidate.exists():
            return ImageFont.truetype(str(candidate), size)
    return ImageFont.load_default()


def field(size: tuple[int, int]) -> Image.Image:
    """The dark brushed field everything sits on."""
    width, height = size
    image = Image.new("RGB", size, PANEL)
    draw = ImageDraw.Draw(image)
    step = max(6, height // 90)
    for y in range(0, height, step):
        shade = 4 if (y // step) % 2 == 0 else 0
        draw.line([(0, y), (width, y)], fill=(PANEL[0] + shade, PANEL[1] + shade, PANEL[2] + shade))
    return image


def reduction(level: float, threshold: float, ratio: float, knee: float) -> float:
    """The static curve, as the engine computes it: decibels taken off."""
    over = level - threshold
    half = knee / 2
    if over <= -half:
        return 0.0
    if over >= half:
        return (1 - 1 / ratio) * over
    into = over + half
    return (1 - 1 / ratio) * into * into / (2 * knee)


def transfer_curve(
    draw: ImageDraw.ImageDraw,
    box: tuple[float, float, float, float],
    stroke: int,
    threshold: float = -24.0,
    ratio: float = 4.0,
    knee: float = 12.0,
) -> None:
    """Input along the bottom, output up the side, both -60 to 0 dB: the
    unity line dashed, the compressed curve over it, the threshold marked."""
    left, top, right, bottom = box
    span = 60.0

    def at(level_in: float, level_out: float) -> tuple[float, float]:
        x = left + (level_in + span) / span * (right - left)
        y = bottom - (level_out + span) / span * (bottom - top)
        return (x, y)

    grid = max(1, stroke // 4)
    for step in range(1, 5):
        level = -span + step * 12
        draw.line([at(level, -span), at(level, 0)], fill=GRID, width=grid)
        draw.line([at(-span, level), at(0, level)], fill=GRID, width=grid)

    dash = 14
    x = left
    while x < right:
        level_a = -span + (x - left) / (right - left) * span
        level_b = -span + (min(right, x + dash) - left) / (right - left) * span
        draw.line([at(level_a, level_a), at(level_b, level_b)], fill=ACCENT_DIM, width=max(2, stroke // 2))
        x += dash * 2

    points = []
    steps = int(right - left)
    for i in range(steps + 1):
        level = -span + i / max(1, steps) * span
        points.append(at(level, level - reduction(level, threshold, ratio, knee)))
    draw.line(points, fill=ACCENT, width=stroke)

    threshold_x, _ = at(threshold, threshold)
    y = top
    while y < bottom:
        draw.line([(threshold_x, y), (threshold_x, min(bottom, y + 8))], fill=STEEL, width=max(1, stroke // 3))
        y += 16


def make_icon() -> None:
    size = (512, 512)
    image = field(size)
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle([28, 28, 484, 484], radius=72, fill=PANEL_LIGHT, outline=STEEL, width=6)
    transfer_curve(draw, (90, 90, 422, 380), stroke=12)
    label = font(64)
    draw.text((256, 436), "CMP", font=label, fill=INK, anchor="mm")
    image.save(OUTPUT / "icon.png")


def make_banner() -> None:
    size = (1600, 400)
    image = field(size)
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle([60, 40, 760, 360], radius=24, fill=PANEL_LIGHT, outline=STEEL, width=3)
    transfer_curve(draw, (110, 70, 710, 330), stroke=8)
    title = font(96)
    draw.text((1180, 160), "RF-COMP", font=title, fill=INK, anchor="mm")
    subtitle = font(34)
    draw.text((1180, 240), "Dynamics · sidechain · parallel blend", font=subtitle, fill=ACCENT, anchor="mm")
    image.save(OUTPUT / "banner.png")


def make_splash() -> None:
    size = (1920, 1080)
    image = field(size)
    draw = ImageDraw.Draw(image)
    title = font(150)
    draw.text((960, 200), "RF-COMP", font=title, fill=INK, anchor="mm")
    subtitle = font(44)
    draw.text((960, 320), "Dynamics · sidechain · parallel blend", font=subtitle, fill=ACCENT, anchor="mm")
    draw.rounded_rectangle([560, 420, 1360, 920], radius=40, fill=PANEL_LIGHT, outline=STEEL, width=4)
    transfer_curve(draw, (620, 470, 1300, 870), stroke=10)
    footer = font(34)
    draw.text(
        (960, 990),
        "live transfer  ·  bounded range  ·  detector listen  ·  stereo link  ·  input, output and reduction meters",
        font=footer,
        fill=MUTED,
        anchor="mm",
    )
    image.save(OUTPUT / "splash.png")


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    make_icon()
    make_banner()
    make_splash()
    for name, expected in (("icon.png", (512, 512)), ("banner.png", (1600, 400)), ("splash.png", (1920, 1080))):
        with Image.open(OUTPUT / name) as image:
            assert image.size == expected, f"{name} is {image.size}, expected {expected}"
            assert image.mode in ("RGB", "RGBA"), f"{name} is {image.mode}"
        print(f"wrote {OUTPUT / name}")


if __name__ == "__main__":
    main()
