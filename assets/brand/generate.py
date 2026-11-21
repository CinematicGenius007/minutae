"""Generate the Minutae brand SVGs (source of truth for every icon).

Run: python3 assets/brand/generate.py
The mark is a geometric lowercase "m" (three stems, two half-circle arches,
butt ends) plus the round dot of the "minutae ●" wordmark.
"""
from pathlib import Path

OUT = Path(__file__).parent
INK, WHITE = "#111111", "#FFFFFF"


def m_path(c1, gap, y_a, y_b):
    r = gap / 2
    a, b, c = c1, c1 + gap, c1 + 2 * gap
    return (f"M{a:g} {y_b:g}V{y_a:g}A{r:g} {r:g} 0 0 1 {b:g} {y_a:g}V{y_b:g}"
            f"M{b:g} {y_a:g}A{r:g} {r:g} 0 0 1 {c:g} {y_a:g}V{y_b:g}")


def mark(scale, cx, cy, dot=True):
    """Mark geometry on a 1024 grid, scaled about (cx, cy) of the mark box."""
    # Base mark (1024 grid): stroke 80, stem gap 150, dot r 48, gap to dot 56.
    s, gap, r_dot, dgap, stem = 80, 150, 48, 56, 200
    width = 2 * gap + s + (dgap + 2 * r_dot if dot else 0)
    height = gap / 2 + s / 2 + stem
    x0, y0 = cx - width * scale / 2, cy - height * scale / 2
    k = scale
    c1 = x0 + (s / 2) * k
    y_a = y0 + (gap / 2 + s / 2) * k
    y_b = y0 + height * k
    parts = [f'<path d="{m_path(c1, gap * k, y_a, y_b)}" fill="none" stroke="{{c}}" stroke-width="{s * k:g}"/>']
    if dot:
        dcx = c1 + 2 * gap * k + (s / 2 + dgap + r_dot) * k
        parts.append(f'<circle cx="{dcx:g}" cy="{y_b - r_dot * k:g}" r="{r_dot * k:g}" fill="{{c}}"/>')
    return "".join(parts)


def svg(w, h, body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg>\n'


def write(name, text):
    (OUT / name).write_text(text)


# App icon, macOS: 824 rounded rect (r 185) centred on a transparent 1024 canvas.
write("app-icon-macos.svg", svg(1024, 1024,
    f'<rect x="100" y="100" width="824" height="824" rx="185" fill="{INK}"/>' + mark(1, 512, 512).replace("{c}", WHITE)))
# App icon, Windows/Linux: full-bleed square, small radius, larger mark.
write("app-icon-square.svg", svg(1024, 1024,
    f'<rect width="1024" height="1024" rx="160" fill="{INK}"/>' + mark(1.2, 512, 512).replace("{c}", WHITE)))
# Favicon: same tile, mark slightly heavier for small sizes.
write("favicon.svg", svg(64, 64,
    f'<rect width="64" height="64" rx="14" fill="{INK}"/>' + mark(64 / 1024 * 1.3, 32, 32).replace("{c}", WHITE)))
# Tray templates (monochrome, alpha only). 32 px = 16 pt @2x.
write("tray-idle.svg", svg(32, 32, mark(32 / 1024 * 1.72, 16, 16).replace("{c}", "#000000")))
write("tray-recording.svg", svg(32, 32,
    '<defs><mask id="k"><rect width="32" height="32" fill="#fff"/>'
    + mark(32 / 1024 * 1.45, 16, 16.4, dot=False).replace("{c}", "#000") +
    '</mask></defs><rect x="1" y="1" width="30" height="30" rx="8" fill="#000" mask="url(#k)"/>'))
# Wordmark-free mark on its own (docs, README).
write("mark.svg", svg(1024, 1024, mark(1.6, 512, 512).replace("{c}", INK)))
print("ok")
