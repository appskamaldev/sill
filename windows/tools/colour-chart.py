"""Writes windows/testdata/colour-chart.png, the colour and range reference for checking the
stream: 1920x1200. The top 800 rows are eight patches of 240 x 800, left to right:
  black (0,0,0), grey 16 (16,16,16), grey 128 (128,128,128), grey 235 (235,235,235),
  white (255,255,255), red (255,0,0), green (0,255,0), blue (0,0,255).
The bottom 400 rows are a grey ramp 0..255, each value 7.5 px wide (x = value * 7.5).

Full-range video shows black ~0 and white ~255 on the device; limited range mistaken for full
shows black ~16 / white ~235 (washed out), the other way round crushes 0..16 and 235..255.
Show it full screen (Photos: F11) and compare a device screenshot's patches.

    python -I windows/tools/colour-chart.py   (needs numpy and opencv-python)
"""
import os
import cv2
import numpy as np

W, H, TOP = 1920, 1200, 800
PATCHES = [(0, 0, 0), (16, 16, 16), (128, 128, 128), (235, 235, 235), (255, 255, 255),
           (255, 0, 0), (0, 255, 0), (0, 0, 255)]
img = np.zeros((H, W, 3), np.uint8)
pw = W // len(PATCHES)
for i, (r, g, b) in enumerate(PATCHES):
    img[:TOP, i * pw:(i + 1) * pw] = (b, g, r)  # OpenCV is BGR
ramp = (np.arange(W) * 256 // W).astype(np.uint8)
img[TOP:, :] = ramp[None, :, None]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "colour-chart.png")
cv2.imwrite(out, img)
print(os.path.normpath(out))
