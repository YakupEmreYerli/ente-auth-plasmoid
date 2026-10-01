#!/usr/bin/env python3
"""Compose docs/banner.png (1280x640, also the GitHub social preview) from an
off-screen render of the popup (tools/preview.py).

    tools/preview.py docs/screenshots --states unlocked --icons ICON_HOME
    tools/banner.py
Needs Pillow and the Noto fonts.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
W, H = 1280, 640
FONT_BOLD = "/usr/share/fonts/noto/NotoSans-Bold.ttf"
FONT_REG = "/usr/share/fonts/noto/NotoSans-Regular.ttf"


def main() -> None:
    bg = Image.new("RGB", (W, H), "#0b0d14")
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    g.ellipse((680, -160, 1400, 520), fill=(142, 92, 255, 70))
    g.ellipse((800, 220, 1360, 800), fill=(29, 185, 84, 45))
    g.ellipse((-260, 320, 460, 940), fill=(90, 120, 255, 26))
    glow = glow.filter(ImageFilter.GaussianBlur(120))
    bg.paste(glow, (0, 0), glow)

    # The render already carries Plasma's dialog frame and a transparent margin.
    shot = Image.open(ROOT / "docs/screenshots/unlocked.png").convert("RGBA")
    shot = shot.crop(shot.getbbox())
    target_h = 580
    shot = shot.resize((round(shot.width * target_h / shot.height), target_h), Image.LANCZOS)
    mask = Image.new("L", shot.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot.width - 1, shot.height - 1), 14, fill=255)
    shot.putalpha(Image.composite(shot.getchannel("A"), mask, mask))
    x, y = W - shot.width - 90, (H - shot.height) // 2
    shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    alpha = shot.getchannel("A").point(lambda a: 150 if a > 0 else 0)
    shadow.paste(Image.new("RGBA", shot.size, (0, 0, 0, 255)), (x, y + 16), alpha)
    shadow = shadow.filter(ImageFilter.GaussianBlur(26))
    bg.paste(shadow, (0, 0), shadow)
    bg.paste(shot, (x, y), shot)

    d = ImageDraw.Draw(bg)
    d.text((88, 196), "Ente Auth Codes", font=ImageFont.truetype(FONT_BOLD, 64), fill="#f2f3fa")
    d.text((92, 282), "for KDE Plasma", font=ImageFont.truetype(FONT_REG, 30), fill="#b89cff")
    body = ImageFont.truetype(FONT_REG, 27)
    lines = [
        ("Your two-factor codes in the panel.", "#a9adc4"),
        ("Search, click, paste.", "#a9adc4"),
        ("Synced with Ente, locked in memory.", "#7d8199"),
    ]
    for i, (line, colour) in enumerate(lines):
        d.text((90, 364 + i * 40), line, font=body, fill=colour)

    out = ROOT / "docs/banner.png"
    bg.save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
