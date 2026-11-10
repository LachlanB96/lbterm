"""How bright a box of a capture.ps1 folder is, frame by frame: a row's flash, a highlight.

  python lbterm/motion/band.py <capture dir> X0 X1 Y0 Y1 SLOWDOWN FROM_MS TO_MS

For each frame whose time of real speed (its time over SLOWDOWN, from the recording's start, the
step capture.ps1 posted) is in FROM_MS..TO_MS: the brightest pixel row's mean luma
(0.2126 R + 0.7152 G + 0.0722 B) inside the box, and which row it was. Put the box clear of the
words (between a row's label and its key, say), so the line it reads is the slab under them.
Not a test: written for the new-tab-menus round's captures of 2026-11-08 (lbterm/FLOWS.md, "A menu,
from what opens it", Seen on screen), where a shortcut's pick and an agenda line's flash were read
with it. A box something else moves through (a new pane's text) reads that too: the row it names
says so.
"""
import csv
import sys
from pathlib import Path

import numpy as np
from PIL import Image

d = Path(sys.argv[1])
x0, x1, y0, y1 = (int(v) for v in sys.argv[2:6])
slow = float(sys.argv[6])
t0, t1 = float(sys.argv[7]), float(sys.argv[8])
for r in csv.DictReader(open(d / 'frames.csv')):
    t = float(r['ms']) / slow
    if t < t0 or t > t1:
        continue
    a = np.asarray(Image.open(d / ('frame%03d.png' % int(r['frame']))).convert('RGB'), dtype=float)[y0:y1, x0:x1]
    m = (0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]).mean(axis=1)
    print(f"{t:7.1f} ms  {m.max():6.1f}  y{int(m.argmax()) + y0}")
