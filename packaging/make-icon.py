import sys
from PIL import Image, ImageFilter
import numpy as np
src, outdir = sys.argv[1], sys.argv[2]
im = Image.open(src).convert("RGBA")
a = np.array(im).astype(np.float32)
al = a[..., 3]
print("alpha 192..255 detail:", np.histogram(al, bins=[192,224,240,248,252,254,255,256])[0])
# 1. alpha: drop the faint haze, make the body fully opaque, keep a short ramp for anti-aliasing
lo, hi = 96.0, 210.0
na = np.clip((al - lo) / (hi - lo), 0, 1)
# 2. edge colour: take colour from the opaque interior nearby (normalised blur), so
#    partly transparent edge pixels carry no orange/red glow fringe
interior = (al >= hi).astype(np.float32)
rgb = a[..., :3]
def blur(x, r):
    img = Image.fromarray(np.clip(x, 0, 255).astype(np.uint8) if x.max() > 1.5 else (x * 255).astype(np.uint8))
    return np.array(img.filter(ImageFilter.GaussianBlur(r))).astype(np.float32)
m = blur(interior, 6) / 255.0
out_rgb = rgb.copy()
for c in range(3):
    num = blur(rgb[..., c] * interior / 255.0, 6) / 255.0 * 255.0
    est = np.where(m > 1e-3, num / np.maximum(m, 1e-3), rgb[..., c])
    edge = (na > 0) & (na < 1)
    out_rgb[..., c] = np.where(edge, est, rgb[..., c])
na = np.where(m < 0.02, 0.0, na)  # haze far from the body: drop it
out = np.dstack([out_rgb, na * 255.0])
out[na == 0] = 0
img = Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")
# 3. crop to the content, square, small margin
ys, xs = np.nonzero(na > 0)
x0, y0, x1, y1 = xs.min(), ys.min(), xs.max(), ys.max()
side = int(max(x1 - x0, y1 - y0) * 1.03)
cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
sq.paste(img.crop((cx - side // 2, cy - side // 2, cx - side // 2 + side, cy - side // 2 + side)), (0, 0))
big = sq.resize((1024, 1024), Image.LANCZOS)
big.save(f"{outdir}/icon.png", optimize=True)
big.resize((256, 256), Image.LANCZOS).save(f"{outdir}/icon-256.png", optimize=True)
sizes = [256, 128, 64, 48, 40, 32, 24, 20, 16]
frames = []
for s in sizes:
    f = big.resize((s, s), Image.LANCZOS)
    if s <= 24:
        from PIL import ImageEnhance
        rgbf = ImageEnhance.Contrast(f.convert("RGB")).enhance(1.25)
        rgbf = ImageEnhance.Color(rgbf).enhance(1.15)
        f = Image.merge("RGBA", (*rgbf.split(), f.split()[3]))
        f = f.filter(ImageFilter.UnsharpMask(radius=0.5, percent=90, threshold=1))
    elif s <= 48:
        f = f.filter(ImageFilter.UnsharpMask(radius=0.6, percent=60, threshold=1))
    frames.append(f)
    f.save(f"{outdir}/preview-{s}.png")
frames[0].save(f"{outdir}/icon.ico", format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[1:])
# previews on light and dark taskbar colours, scaled up with nearest-neighbour
for s in [16, 24, 32, 48]:
    f = Image.open(f"{outdir}/preview-{s}.png")
    row = Image.new("RGBA", (s * 2 + 6, s + 4), (0, 0, 0, 0))
    for i, bg in enumerate([(32, 32, 32, 255), (243, 243, 243, 255)]):
        tile = Image.new("RGBA", (s + 2, s + 2), bg)
        tile.alpha_composite(f, (1, 1))
        row.paste(tile, (i * (s + 4), 1))
    row.resize((row.width * 8, row.height * 8), Image.NEAREST).save(f"{outdir}/zoom-{s}.png")
# fringe check on large: composite on white and on black
for name, bg in [("white", (255, 255, 255, 255)), ("black", (0, 0, 0, 255))]:
    c = Image.new("RGBA", big.size, bg); c.alpha_composite(big); c.convert("RGB").save(f"{outdir}/on-{name}.jpg", quality=85)
print("ok", side)
