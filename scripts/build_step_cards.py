#!/usr/bin/env python3
"""Build the setup-step cards for the app's welcome screen and for sovatela.eu.

The sources in assets/ are 1254² renders, each a 3D icon over its own tile with
a title and subtitle baked into the picture. They cannot be used as they are:
the captions differ from the wording the product actually uses, and every render
frames its tile differently, so a row of them does not read as a set.

So this rebuilds them. From each source it lifts just the artwork, then draws one
shared container — same geometry, same bevel, same shadow, same background for
all of them — centres the artwork inside it, and renders the step's own wording
underneath in Inter, the typeface the rest of the product is set in. The result
is a set that matches because it was drawn to, rather than because five separate
renders happened to agree.

The artwork is colour-shifted so its own background matches the container fill
exactly, then feathered at the edges, which is what keeps the pasted square from
showing as a patch.

    ./scripts/build_step_cards.py            # writes app + site cards
    ./scripts/build_step_cards.py --preview  # also writes a strip to look at

Needs Pillow, numpy and fontTools (the Inter binary is converted out of
node_modules, so the cards are set in the same font the app ships).
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFilter, ImageFont

REPO = Path(__file__).resolve().parent.parent
SRC = REPO / "assets"
APP_OUT = REPO / "src/assets/steps"
WEB_OUT = REPO / "deploy/web/steps"
FONT_SRC = REPO / "node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2"

# Where each source's baked-in caption starts. Measured by scanning for the
# first row of text-like pixels below the halfway line — not guessed, and not
# transferable to a re-render. Everything below this is discarded.
CAPTION_TOP = {
    "download": 1005,
    "createAccount": 830,
    "genKey": 824,
    "pasteKey": 1020,
    "chat": 1070,
}

# The wording belongs to each surface, not to the pictures: the app is already
# installed so it never says "Download", and the site can name scaleway.com
# because a reader can click it there.
APP_STEPS = [
    ("createAccount", "Create a Scaleway account", "Free to open"),
    ("genKey", "Generate an API key", "Shown once — copy it"),
    ("pasteKey", "Paste it into Sovatela", "Straight into your keychain"),
    ("chat", "Start chatting", "That's the whole setup"),
]
# Three cards, not five. The last two said what happens *after* the key exists —
# paste it in, then chat — which is the part a reader cannot act on until they
# have downloaded anything, and it pushed the download buttons below the fold.
# That wording now sits in prose under the strip, where it can be specific about
# the one thing that actually catches people out: Scaleway shows the key once.
# "Generate an API key" matches the label on Scaleway's own button.
WEB_STEPS = [
    ("download", "Download the app", "macOS, Windows or Linux"),
    ("createAccount", "Get a Scaleway account", "Free at scaleway.com"),
    ("genKey", "Generate an API key", "Copy it, paste it into the app"),
]

# Card geometry, in the 1024² space everything is composed at.
SIZE = 1024
MARGIN = 40  # breathing room around the tile, where its shadow falls
RADIUS = 132
TILE_FILL = (26, 15, 48)  # the dark tile face
BACKDROP = (33, 15, 78)  # the deep purple behind it, as in the sources
EDGE = (128, 96, 214)  # top bevel highlight
TITLE_RGB = (255, 255, 255)
SUB_RGB = (183, 163, 238)
ART_BOX = 0.44  # artwork fits a square this fraction of the card, so a tall
                # drawing and a wide one carry the same weight
ART_CENTER_Y = 0.30  # where the artwork sits vertically
TITLE_Y = 0.575
# Type is sized so that a card shown ~180px wide in the strip renders its title
# at about 15px and its note at about 12px — the sizes these two lines had when
# they were HTML. Baked type cannot reflow or scale, so it has to be set for the
# size it will actually be read at, not for how the card looks zoomed in.
TITLE_PX, TITLE_LEADING = 86, 100
SUB_PX, SUB_LEADING = 66, 78
OUT_PX = 640


def inter(weight: str, size: int) -> ImageFont.FreeTypeFont:
    """Inter at a named weight — the same font binary the app bundles.

    Pillow needs a file it can open, and the shipped font is woff2, so it is
    unpacked once into the temp directory rather than left lying in the repo.
    """
    cache = Path(tempfile.gettempdir()) / "sovatela-inter.ttf"
    if not cache.exists():
        f = TTFont(FONT_SRC)
        f.flavor = None
        f.save(cache)
    font = ImageFont.truetype(str(cache), size)
    font.set_variation_by_name(weight)
    return font


def artwork(name: str) -> Image.Image:
    """The artwork alone: everything above the caption, cropped to what is drawn."""
    im = Image.open(SRC / f"{name}.png").convert("RGB")
    cut = CAPTION_TOP[name]
    region = im.crop((0, 0, im.width, cut))
    grey = np.asarray(region.convert("L"), dtype=float)
    # Drawing is anything that departs from the background in EITHER direction:
    # several sources stand bright elements on a panel darker than the frame,
    # and a brightness-only mask cuts straight through those panels.
    base = np.median(grey)
    mask = Image.fromarray(((np.abs(grey - base) > 22) * 255).astype(np.uint8))
    # Open the mask so sparkles and glow specks do not inflate the bounds.
    mask = mask.filter(ImageFilter.MinFilter(9)).filter(ImageFilter.MaxFilter(9))
    m = np.asarray(mask) > 127
    ys, xs = np.where(m)
    pad = 18
    box = (
        max(0, int(xs.min()) - pad),
        max(0, int(ys.min()) - pad),
        min(region.width, int(xs.max()) + 1 + pad),
        min(cut, int(ys.max()) + 1 + pad),
    )
    patch = np.asarray(region.crop(box), dtype=float)
    # Shift the patch so whatever surrounds the drawing becomes exactly the tile
    # it is about to sit on. Several sources stand their artwork on an inner
    # tile darker than the frame, so the estimate must come from the pixels
    # inside this crop that are not drawing — measure it from the frame, or from
    # a border ring the wide drawings run straight through, and the paste shows
    # as a dark rectangle.
    behind = ~m[box[1]:box[3], box[0]:box[2]]
    patch_bg = np.median(patch[behind], axis=0)
    shifted = np.clip(patch + (np.array(TILE_FILL) - patch_bg), 0, 255).astype(np.uint8)
    art = Image.fromarray(shifted)

    # Dissolve the edges into the tile: a rounded, generously blurred alpha, so
    # neither a corner nor a leftover gradient draws a line on the container.
    alpha = Image.new("L", art.size, 0)
    inset = max(3, int(min(art.size) * 0.06))
    ImageDraw.Draw(alpha).rounded_rectangle(
        (inset, inset, art.width - inset, art.height - inset), inset * 2, fill=255
    )
    art.putalpha(alpha.filter(ImageFilter.GaussianBlur(inset * 1.4)))
    return art


def tile() -> Image.Image:
    """The shared container: one rounded 3D face, drawn identically every time."""
    card = Image.new("RGB", (SIZE, SIZE), BACKDROP)
    box = (MARGIN, MARGIN, SIZE - MARGIN, SIZE - MARGIN)

    # Cast shadow first, so the face sits on top of it.
    shadow = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(shadow).rounded_rectangle(
        (box[0], box[1] + 18, box[2], box[3] + 22), RADIUS, fill=150
    )
    card = Image.composite(
        Image.new("RGB", (SIZE, SIZE), (14, 6, 34)), card,
        shadow.filter(ImageFilter.GaussianBlur(26)),
    )

    face = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    fd = ImageDraw.Draw(face)
    fd.rounded_rectangle(box, RADIUS, fill=(*TILE_FILL, 255))
    # One quiet edge all the way round is the whole of the 3D read. An explicit
    # top-arc highlight was tried and read as a scratch across the card.
    fd.rounded_rectangle(box, RADIUS, outline=(*EDGE, 42), width=2)
    return Image.alpha_composite(card.convert("RGBA"), face).convert("RGB")


def lit(card: Image.Image) -> Image.Image:
    """The soft light inside the top of the face, as in the renders.

    Applied *after* the artwork is placed, not before: a flat patch of tile
    colour sitting under a lit tile shows its own edges as a square halo. Light
    falling across both is what makes the two read as one surface.
    """
    glow = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(glow).ellipse(
        (SIZE * 0.18, -SIZE * 0.22, SIZE * 0.82, SIZE * 0.42), fill=42
    )
    return Image.composite(
        Image.new("RGB", (SIZE, SIZE), (58, 34, 116)), card,
        glow.filter(ImageFilter.GaussianBlur(90)),
    )


def wrap(draw, text, font, max_width):
    lines, line = [], ""
    for word in text.split():
        trial = f"{line} {word}".strip()
        if draw.textlength(trial, font=font) <= max_width or not line:
            line = trial
        else:
            lines.append(line)
            line = word
    if line:
        lines.append(line)
    return lines


def card(name: str, title: str, subtitle: str) -> Image.Image:
    im = tile()
    art = artwork(name)
    fit = SIZE * ART_BOX
    scale = min(fit / art.width, fit / art.height)
    w, h = int(art.width * scale), int(art.height * scale)
    art = art.resize((w, h), Image.LANCZOS)
    im.paste(art, ((SIZE - w) // 2, int(SIZE * ART_CENTER_Y - h / 2)), art)
    im = lit(im)  # light the tile and the artwork together, so they read as one

    d = ImageDraw.Draw(im)
    inner = SIZE - MARGIN * 2 - 96
    title_font = inter("SemiBold", TITLE_PX)
    sub_font = inter("Regular", SUB_PX)

    y = SIZE * TITLE_Y
    for line in wrap(d, title, title_font, inner):
        d.text((SIZE / 2, y), line, font=title_font, fill=TITLE_RGB, anchor="ma")
        y += TITLE_LEADING
    y += 16
    for line in wrap(d, subtitle, sub_font, inner):
        d.text((SIZE / 2, y), line, font=sub_font, fill=SUB_RGB, anchor="ma")
        y += SUB_LEADING
    return im


def build(steps, out_dir: Path) -> list[Image.Image]:
    out_dir.mkdir(parents=True, exist_ok=True)
    made = []
    for name, title, subtitle in steps:
        im = card(name, title, subtitle).resize((OUT_PX, OUT_PX), Image.LANCZOS)
        path = out_dir / f"{name}.webp"
        im.save(path, "WEBP", quality=88, method=6)
        print(f"  {path.relative_to(REPO)}  {path.stat().st_size // 1024}KB")
        made.append(im)
    return made


if __name__ == "__main__":
    print("App welcome screen:")
    app = build(APP_STEPS, APP_OUT)
    print("Site (sovatela.eu):")
    web = build(WEB_STEPS, WEB_OUT)

    if "--preview" in sys.argv:
        for tag, images in (("app", app), ("web", web)):
            gap, w = 16, 260
            strip = Image.new(
                "RGB", (len(images) * (w + gap) + gap, w + gap * 2), (18, 16, 24)
            )
            for i, im in enumerate(images):
                strip.paste(im.resize((w, w), Image.LANCZOS), (gap + i * (w + gap), gap))
            path = Path(sys.argv[sys.argv.index("--preview") + 1]) / f"strip-{tag}.png"
            strip.save(path)
            print(f"preview: {path}")
