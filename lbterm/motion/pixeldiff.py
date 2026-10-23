"""Pixel diff of two captures of the same size.

usage: python pixeldiff.py A.png B.png OUT.png

Writes OUT with the differing pixels red over a dimmed A, and prints how many differ, where,
and each one's colours when there are only a few (so a one-pixel seam can be read off).
"""
import sys

import numpy as np
from PIL import Image

a_path, b_path, out_path = sys.argv[1:4]
a = np.asarray(Image.open(a_path).convert("RGB"), dtype=int)
b = np.asarray(Image.open(b_path).convert("RGB"), dtype=int)
h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
if a.shape[:2] != b.shape[:2]:
    print(f"sizes differ: {a.shape[1]}x{a.shape[0]} and {b.shape[1]}x{b.shape[0]}; comparing {w}x{h}")
a, b = a[:h, :w], b[:h, :w]
mask = np.abs(a - b).sum(axis=2) > 6
out = (a // 2).astype(np.uint8)
out[mask] = (255, 0, 0)
Image.fromarray(out).save(out_path)
n = int(mask.sum())
print(f"differing pixels: {n} of {w * h}")
if n:
    ys, xs = np.nonzero(mask)
    print(f"within x {xs.min()}..{xs.max()}, y {ys.min()}..{ys.max()}")
    if n <= 40:
        for y, x in zip(ys, xs):
            print(f"  ({x}, {y}): {tuple(int(c) for c in a[y, x])} vs {tuple(int(c) for c in b[y, x])}")
