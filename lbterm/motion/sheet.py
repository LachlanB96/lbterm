"""Contact sheet, edge plot and spring fit for frames from capture.ps1.

usage: python sheet.py DIR [--row Y] [--from X] [--bright B | --ring] [--count N] [--cols C]
                           [--expect FROM TO MS BOUNCE] [--clamp MAX]

Reads DIR/frame*.png and DIR/frames.csv (frame, ms). Writes:
  DIR/contact.png  the frames around the motion, tiled, each with its time and edge
  DIR/edge.csv     per frame, the x of the first pixel on row Y at least B bright (a split
                   line, found even while its pane is still fading in)
  DIR/edge.png     that x over time, and with --expect the spring it should follow
With --ring the edge is instead just past the ring's right-hand stroke on row Y, a row of the tab
panel's empty card (panel-demo.ps1): the end of the second run of stroke-bright pixels from the
left (any channel over 100), so it's found grey at rest, red open, or mid-fade, and the frame round
the panes further right doesn't count. The light round the ring is too faint to. The motion then
starts when the edge first moves. --clamp MAX compares the spring clamped at MAX, for an edge
that can't be seen past some point.
With --expect it prints the start that fits best and how far the frames are from the spring:
a split's line on the layout spring lands within a few pixels (capture timing, not motion).
MS is the spring's duration as captured, so already stretched by any slowdown.
"""
import argparse
import csv
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument("dir")
ap.add_argument("--row", type=int, default=200)
# Where along the row to start looking: past the tab panel and the first card's own edge, so the
# edge found is a split's gutter, where the cards on either side of it meet
ap.add_argument("--from", dest="start", type=int, default=0)
ap.add_argument("--bright", type=float, default=30)
ap.add_argument("--count", type=int, default=24)
ap.add_argument("--cols", type=int, default=6)
ap.add_argument("--expect", type=float, nargs=4, metavar=("FROM", "TO", "MS", "BOUNCE"))
ap.add_argument("--ring", action="store_true", help="measure the tab panel's red ring")
ap.add_argument("--clamp", type=float)
# The edge is on screen before it moves (an outline gliding): the motion starts when it moves more
# than 2 px (its soft pixel flickers across the brightness threshold at rest)
ap.add_argument("--still", action="store_true")
args = ap.parse_args()
body = args.ring or None
d = Path(args.dir)

rows = list(csv.DictReader(open(d / "frames.csv")))
times = [float(r["ms"]) for r in rows]
frames = [
    np.asarray(Image.open(d / f"frame{int(r['frame']):03d}.png").convert("RGB"), dtype=int)
    for r in rows
]


def edge(img):
    if body is not None:
        stroke = img[args.row][:, :3].max(axis=1) > 100
        # Runs of stroke pixels: the ring's left side, then its right
        starts = np.nonzero(stroke & ~np.r_[False, stroke[:-1]])[0]
        ends = np.nonzero(stroke & ~np.r_[stroke[1:], False])[0]
        return int(ends[1]) + 1 if len(ends) >= 2 else None
    # Left of the scrollbar column, which is bright too
    line = img[args.row][args.start : img.shape[1] - 16]
    hit = np.nonzero(line.mean(axis=1) >= args.bright)[0]
    return args.start + int(hit[0]) if len(hit) else None


edges = [edge(f) for f in frames]
with open(d / "edge.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["frame", "ms", "edge_x"])
    for i, (t, e) in enumerate(zip(times, edges)):
        w.writerow([i, t, "" if e is None else e])

seen = [i for i, e in enumerate(edges) if e is not None]
if not seen:
    raise SystemExit(f"nothing at least {args.bright} bright on row {args.row} in any frame")
first = seen[0]
if body is not None or args.still:
    # The panel's edge is there from the start: the motion starts when it moves
    first = next((i for i in seen if abs(edges[i] - edges[seen[0]]) > 2), seen[0])
final = edges[-1]
settled = next((i for i in seen if all(edges[j] == final for j in range(i, len(edges)))), seen[-1])
print(
    f"edge first seen at frame {first} ({times[first]:.0f} ms) x={edges[first]}; "
    f"settled at frame {settled} ({times[settled]:.0f} ms) x={final}"
)

lo = max(0, first - 2)
hi = min(len(frames) - 1, settled + 3)
pick = sorted(set(np.linspace(lo, hi, min(args.count, hi - lo + 1)).round().astype(int)))
tw, th = frames[0].shape[1] // 3, frames[0].shape[0] // 3
sheet = Image.new("RGB", (args.cols * tw, math.ceil(len(pick) / args.cols) * (th + 16)), (40, 40, 40))
draw = ImageDraw.Draw(sheet)
for n, i in enumerate(pick):
    x, y = (n % args.cols) * tw, (n // args.cols) * (th + 16)
    thumb = Image.fromarray(frames[i].astype(np.uint8)).resize((tw, th), Image.LANCZOS)
    sheet.paste(thumb, (x, y + 16))
    label = f"#{i} {times[i] - times[first]:+.0f} ms" + ("" if edges[i] is None else f"  x={edges[i]}")
    draw.text((x + 4, y + 2), label, fill=(230, 230, 230))
sheet.save(d / "contact.png")

W, H, pad = 900, 420, 50
plot = Image.new("RGB", (W, H), (250, 250, 250))
pd = ImageDraw.Draw(plot)
t0, t1 = times[lo], times[hi]
xs = [e for e in edges[lo : hi + 1] if e is not None]
ylo, yhi = min(xs), max(xs)
if args.expect:
    ylo = min(ylo, args.expect[0], args.expect[1])
    yhi = max(yhi, args.expect[0], args.expect[1])
yhi += 1


def px(t, v):
    return (
        pad + (t - t0) / (t1 - t0) * (W - 2 * pad),
        H - pad - (v - ylo) / (yhi - ylo) * (H - 2 * pad),
    )


pd.rectangle([pad, pad, W - pad, H - pad], outline=(180, 180, 180))
what = "panel edge" if body is not None else "split line"
title = f"{what} x on row {args.row}: measured (red)" + (", spring (blue)" if args.expect else "")
pd.text((pad, 10), title, fill=(0, 0, 0))
pd.text((4, pad - 6), str(int(yhi - 1)), fill=(0, 0, 0))
pd.text((4, H - pad - 6), str(int(ylo)), fill=(0, 0, 0))
pd.text((pad, H - pad + 8), f"{t0 - times[first]:.0f} ms", fill=(0, 0, 0))
pd.text((W - pad - 40, H - pad + 8), f"{t1 - times[first]:.0f} ms", fill=(0, 0, 0))

if args.expect:
    x_from, x_to, ms, bounce = args.expect
    omega = 2 * math.pi / (ms / 1000)
    zeta = 1 - bounce

    def spring(t):
        x = free_spring(t)
        return min(x, args.clamp) if args.clamp is not None else x

    def free_spring(t):
        # lbterm-motion's Spring::state from rest, as in the config's spring(MS, BOUNCE)
        if t <= 0:
            return x_from
        x0 = x_from - x_to
        if abs(zeta - 1) < 1e-9:
            return x_to + x0 * math.exp(-omega * t) * (1 + omega * t)
        wd = omega * math.sqrt(1 - zeta * zeta)
        e = math.exp(-zeta * omega * t)
        return x_to + e * (x0 * math.cos(wd * t) + zeta * omega * x0 / wd * math.sin(wd * t))

    # The motion starts at a DComp commit the capture can't see: take the
    # start that fits best, then say how far the frames are from the spring
    measured = [(times[i], edges[i]) for i in range(lo, hi + 1) if edges[i] is not None]

    def rms(start):
        return math.sqrt(sum((spring((t - start) / 1000) - e) ** 2 for t, e in measured) / len(measured))

    start = min(np.arange(times[first] - 400, times[first] + 1, 0.5), key=rms)
    worst = max(abs(spring((t - start) / 1000) - e) for t, e in measured)
    print(
        f"spring fit: start {start - times[first]:+.1f} ms from first sighting, "
        f"rms {rms(start):.2f} px, worst {worst:.2f} px over {len(measured)} frames"
    )
    last = None
    for k in range(400):
        t = t0 + (t1 - t0) * k / 399
        p = px(t, spring((t - start) / 1000))
        if last:
            pd.line([last, p], fill=(40, 90, 220), width=2)
        last = p

for i in range(lo, hi + 1):
    if edges[i] is not None:
        x, y = px(times[i], edges[i])
        pd.ellipse([x - 3, y - 3, x + 3, y + 3], fill=(220, 40, 40))
plot.save(d / "edge.png")
print(f"contact sheet {d / 'contact.png'}, plot {d / 'edge.png'}")
