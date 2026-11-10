"""A menu card's edge, frame by frame, in a motion/capture.ps1 folder: out from the card's right
side at its middle (over the pane's blank rows), each pixel's luma above the last frame's (at
rest, long after the motion; the first frame can already show it) at each distance from the
card's fill (-4 inside it, 0 the hairline, then its light), and for the fill, the hairline and
the light 2, 4, 6 and 9 px out how far they lifted, when, and how long they stayed above half
and a tenth of that. Times are from the recording's start, where capture.ps1 posts -Cli's step.
The card is the state file's menu.rect, or --rect X,Y,W,H when the menu closed in the run.
Written for the key's flare (lbterm/FLOWS.md, "A menu, from what opens it to the end of its
fade", Seen on screen, "The key's flare"):

  pwsh -File lbterm/motion/capture.ps1 -Name bump-1x -Slowdown 1 -Seconds 1.5 -Setup post,rclick,row,0 -Cli post,key,Q
  python lbterm/motion/menu-edge.py target/motion/bump-1x [--profile] [--rect X,Y,W,H] [--key-ms MS]

--profile prints the first frame's pixels along the line instead.
"""
import csv, json, sys
from pathlib import Path
import numpy as np
from PIL import Image

d = Path(sys.argv[1])
profile = '--profile' in sys.argv
key_ms = None
if '--key-ms' in sys.argv:
    key_ms = float(sys.argv[sys.argv.index('--key-ms') + 1])
state = json.loads((d / 'state.json').read_text())
w = state['windows'][0]
menu = w.get('menu')
if not menu:
    # The card's place from the run's own last frame isn't in a closed menu's state: take it
    # from --rect x,y,w,h
    r = [int(v) for v in sys.argv[sys.argv.index('--rect') + 1].split(',')]
    rect = dict(x=r[0], y=r[1], w=r[2], h=r[3])
else:
    rect = menu['rect']
times = {}
with open(d / 'frames.csv') as f:
    for row in csv.DictReader(f):
        times[int(row['frame'])] = float(row['ms'])
frames = sorted(d.glob('frame*.png'))

def luma(p):
    r, g, b = [float(c) for c in p[:3]]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b

x0 = rect['x'] + rect['w']
y = rect['y'] + rect['h'] // 2
# Distances from the fill's right edge: -4 is inside the card, 0 the hairline, then the light
dists = list(range(-4, 15))
rows = []
for i, path in enumerate(frames):
    a = np.asarray(Image.open(path).convert('RGB'))
    px = {k: a[y, x0 + k] for k in dists}
    rows.append((times.get(i, 0.0), px))

if profile:
    for k in dists:
        print(f"{k:+3d}: {tuple(int(c) for c in rows[0][1][k])} luma {luma(rows[0][1][k]):.1f}")
    sys.exit(0)

# At rest: the last frame, long after the flare (the first can already show it)
rest = {k: luma(rows[-1][1][k]) for k in dists}
show = [-4, 0, 1, 2, 4, 6, 8, 10, 12]
print('ms      ' + ' '.join(f"{k:+3d}  " for k in show) + '  (luma above the first frame)')
for t, px in rows:
    print(f"{t:7.1f} " + ' '.join(f"{luma(px[k]) - rest[k]:+5.1f}" for k in show))
# The summary: for the hairline (0) and the light 2 and 6 px out, the peak, when, and how long
# above half and a tenth of its rise
print()
for k in [-4, 0, 2, 4, 6, 9]:
    lift = [(t, luma(px[k]) - rest[k]) for t, px in rows]
    peak_t, peak = max(lift, key=lambda p: p[1])
    if peak < 3:
        print(f"{k:+3d} px: no lift (peak {peak:+.1f})")
        continue
    above = lambda share: [t for t, v in lift if v >= share * peak]
    half, tenth = above(0.5), above(0.1)
    rgb_rest = tuple(int(c) for c in rows[-1][1][k])
    rgb_peak = tuple(int(c) for c in [px for t, px in rows if t == peak_t][0][k])
    # The key is posted as the recording starts, at 0 ms
    start = key_ms if key_ms is not None else 0.0
    print(f"{k:+3d} px: {rgb_rest} luma {rest[k]:.1f} -> {rgb_peak} (+{peak:.1f}, {peak / max(rest[k], 1) * 100:.0f}% brighter) at {peak_t - start:.0f} ms;"
          f" above half from {half[0] - start:.0f} to {half[-1] - start:.0f} ms ({half[-1] - half[0]:.0f}),"
          f" above a tenth from {tenth[0] - start:.0f} to {tenth[-1] - start:.0f} ms ({tenth[-1] - tenth[0]:.0f})")
