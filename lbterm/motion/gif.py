"""An animated GIF of frames from capture.ps1 or panel-demo.ps1, at real speed.

usage: python gif.py DIR [--out FILE] [--crop X0,Y0,X1,Y1] [--scale S] [--step MS]
                         [--slowdown K] [--hold MS]

Reads DIR/frame*.png and DIR/frames.csv (frame, ms). Every STEP ms of real time (the frames'
times divided by --slowdown, the slowdown they were taken at) it shows the last frame taken by
then, so the GIF plays at the speed the motion really has. --hold pauses on the first and last
frames. One palette for every frame, so nothing flickers between them.
"""
import argparse
import csv
from pathlib import Path

from PIL import Image

ap = argparse.ArgumentParser()
ap.add_argument("dir")
ap.add_argument("--out")
ap.add_argument("--crop")
ap.add_argument("--scale", type=float, default=1.0)
ap.add_argument("--step", type=float, default=20)
ap.add_argument("--slowdown", type=float, default=1.0)
ap.add_argument("--hold", type=float, default=700)
args = ap.parse_args()
d = Path(args.dir)
out = Path(args.out) if args.out else d / "demo.gif"

rows = list(csv.DictReader(open(d / "frames.csv")))
times = [float(r["ms"]) / args.slowdown for r in rows]
crop = tuple(int(v) for v in args.crop.split(",")) if args.crop else None


def load(i):
    img = Image.open(d / f"frame{int(rows[i]['frame']):03d}.png").convert("RGB")
    if crop:
        img = img.crop(crop)
    if args.scale != 1.0:
        img = img.resize((round(img.width * args.scale), round(img.height * args.scale)), Image.LANCZOS)
    return img


# Which frame shows at each tick, runs of the same frame joined into one
picks = []
t, i = times[0], 0
while t <= times[-1]:
    while i + 1 < len(times) and times[i + 1] <= t:
        i += 1
    if picks and picks[-1][0] == i:
        picks[-1][1] += args.step
    else:
        picks.append([i, args.step])
    t += args.step
picks[0][1] += args.hold
picks[-1][1] += args.hold

images = {i: load(i) for i, _ in picks}
# One palette, taken from frames across the whole run
sample = [images[picks[k][0]] for k in range(0, len(picks), max(1, len(picks) // 6))]
strip = Image.new("RGB", (sample[0].width, sample[0].height * len(sample)))
for k, img in enumerate(sample):
    strip.paste(img, (0, k * img.height))
palette = strip.quantize(colors=255, method=Image.Quantize.MEDIANCUT)
frames = [images[i].quantize(palette=palette, dither=Image.Dither.NONE) for i, _ in picks]
# GIF delays are in hundredths of a second
durations = [max(20, round(ms / 10) * 10) for _, ms in picks]
frames[0].save(out, save_all=True, append_images=frames[1:], duration=durations, loop=0, disposal=1)
real = sum(ms for _, ms in picks) - 2 * args.hold
print(f"{out}: {len(frames)} frames from {len(rows)}, {real:.0f} ms of motion, "
      f"{out.stat().st_size / 1e6:.1f} MB")
