"""Draws the app icon (assets/icon-*.png, assets/icon.ico) and a Windows resource file (assets/app.res)."""
import math, struct, io
from PIL import Image, ImageDraw

def draw(size):
    S = size * 4
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    pad = S * 0.04
    d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=S * 0.2, fill=(20, 23, 26, 255), outline=(52, 58, 64, 255), width=max(2, S // 64))
    cx = cy = S / 2
    amber = (242, 179, 61, 255)
    r = S * 0.17
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=amber)
    for i in range(8):
        a = i * math.pi / 4
        r0, r1, w = S * 0.27, S * 0.38, S * 0.045
        x0, y0 = cx + math.cos(a) * r0, cy + math.sin(a) * r0
        x1, y1 = cx + math.cos(a) * r1, cy + math.sin(a) * r1
        d.line([x0, y0, x1, y1], fill=amber, width=int(w * (1.25 if i % 2 == 0 else 1)))
        for (x, y) in ((x0, y0), (x1, y1)):
            rr = w * (0.62 if i % 2 == 0 else 0.5)
            d.ellipse([x - rr, y - rr, x + rr, y + rr], fill=amber)
    return im.resize((size, size), Image.LANCZOS)

sizes = [16, 24, 32, 48, 64, 128, 256]
imgs = {s: draw(s) for s in sizes}
imgs[256].save("assets/icon-256.png"); imgs[32].save("assets/icon-32.png"); imgs[64].save("assets/icon-64.png")

# --- ICO (PNG-compressed entries) ---
blobs = []
for s in sizes:
    b = io.BytesIO(); imgs[s].save(b, "PNG"); blobs.append((s, b.getvalue()))
ico = struct.pack("<HHH", 0, 1, len(blobs))
off = 6 + 16 * len(blobs)
for s, data in blobs:
    ico += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), off); off += len(data)
ico += b"".join(d for _, d in blobs)
open("assets/icon.ico", "wb").write(ico)

# --- .res with RT_GROUP_ICON (14) + RT_ICON (3) entries ---
def entry(rtype, rid, data, lang=0x0409):
    hdr = struct.pack("<II", len(data), 32)
    hdr += struct.pack("<HH", 0xFFFF, rtype) + struct.pack("<HH", 0xFFFF, rid)
    hdr += struct.pack("<IHHII", 0, 0x1030, lang, 0, 0)
    out = hdr + data
    return out + b"\0" * (-len(out) % 4)

res = entry(0, 0, b"")[:0]
res = struct.pack("<II", 0, 32) + struct.pack("<HHHH", 0xFFFF, 0, 0xFFFF, 0) + struct.pack("<IHHII", 0, 0, 0, 0, 0)
grp = struct.pack("<HHH", 0, 1, len(blobs))
for i, (s, data) in enumerate(blobs, start=1):
    grp += struct.pack("<BBBBHHIH", s % 256, s % 256, 0, 0, 1, 32, len(data), i)
    res += entry(3, i, data)
res += entry(14, 1, grp)
open("assets/app.res", "wb").write(res)
print("ok", len(res))
