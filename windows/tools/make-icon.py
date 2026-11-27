"""Draws windows/host/assets/sill.ico (16-256 px, PNG entries) and sill-256.png: the Sill mark, a
sky tile (#60CDFF) with a white window frame resting on a bold sill. Supersampled, so small sizes
stay crisp.   python -I windows/tools/make-icon.py   (numpy, opencv-python)"""
import io, os, struct
import cv2
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "host", "assets")
SKY = (0x60, 0xCD, 0xFF)        # RGB
INK = (0x00, 0x1E, 0x2E)


def rrect(mask, x0, y0, x1, y1, r):
    cv2.rectangle(mask, (x0 + r, y0), (x1 - r, y1), 255, -1)
    cv2.rectangle(mask, (x0, y0 + r), (x1, y1 - r), 255, -1)
    for cx, cy in ((x0 + r, y0 + r), (x1 - r, y0 + r), (x0 + r, y1 - r), (x1 - r, y1 - r)):
        cv2.circle(mask, (cx, cy), r, 255, -1, lineType=cv2.LINE_AA)


def draw(size):
    S = 1024
    tile = np.zeros((S, S), np.uint8)
    rrect(tile, 0, 0, S - 1, S - 1, int(S * 0.22))
    mark = np.zeros((S, S), np.uint8)
    # Window frame: an outlined rounded rect with a centre mullion.
    t = int(S * (0.075 if size >= 32 else 0.10))
    x0, y0, x1, y1 = int(S * 0.24), int(S * 0.20), int(S * 0.76), int(S * 0.66)
    rrect(mark, x0, y0, x1, y1, int(S * 0.06))
    inner = np.zeros_like(mark)
    rrect(inner, x0 + t, y0 + t, x1 - t, y1 - t, max(1, int(S * 0.06) - t // 2))
    mark[inner > 0] = 0
    if size >= 32:
        cx = (x0 + x1) // 2
        cv2.rectangle(mark, (cx - t // 2, y0), (cx + t // 2, y1), 255, -1)
    # The sill: a wider bar under the window.
    rrect(mark, int(S * 0.16), int(S * 0.72), int(S * 0.84), int(S * 0.72) + int(S * 0.11), int(S * 0.04))
    rgba = np.zeros((S, S, 4), np.uint8)
    rgba[..., 0:3] = SKY[::-1]  # BGR
    white = mark > 0
    rgba[white, 0:3] = (255, 255, 255)
    rgba[..., 3] = tile
    small = cv2.resize(rgba, (size, size), interpolation=cv2.INTER_AREA)
    return small


def png(img):
    ok, buf = cv2.imencode(".png", img)
    return buf.tobytes()


sizes = [16, 20, 24, 32, 48, 64, 256]
images = [png(draw(s)) for s in sizes]
head = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
entries = b""
for s, data in zip(sizes, images):
    entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
os.makedirs(OUT, exist_ok=True)
with open(os.path.join(OUT, "sill.ico"), "wb") as f:
    f.write(head + entries + b"".join(images))
with open(os.path.join(OUT, "sill-256.png"), "wb") as f:
    f.write(images[-1])
print("wrote", os.path.normpath(os.path.join(OUT, "sill.ico")))
