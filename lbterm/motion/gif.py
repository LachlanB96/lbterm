"""An animated GIF of frames from capture.ps1, panel-demo.ps1 or a demo script, at real speed.

usage: python gif.py DIR [--out FILE] [--crop X0,Y0,X1,Y1] [--scale S] [--step MS]
                         [--slowdown K] [--hold MS] [--pointer [--pointer-scale S]]

Reads DIR/frame*.png and DIR/frames.csv (frame, ms). Every STEP ms of real time (the frames'
times divided by --slowdown, the slowdown they were taken at) it shows the last frame taken by
then, so the GIF plays at the speed the motion really has. --hold pauses on the first and last
frames. One palette for every frame, so nothing flickers between them.

--pointer draws an arrow where a posted pointer was in each frame, from frames.csv's x and y
columns (client pixels, -1 while it was off the window; strips-demo.ps1 writes them): posted
messages move no real pointer, and captures have none, so without it a card opens with nothing
to say why. --pointer-scale sizes it to the window's DPI (1.25 at 120).
"""
import argparse
import csv
from pathlib import Path

from PIL import Image, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument("dir")
ap.add_argument("--out")
ap.add_argument("--crop")
ap.add_argument("--scale", type=float, default=1.0)
ap.add_argument("--step", type=float, default=20)
ap.add_argument("--slowdown", type=float, default=1.0)
ap.add_argument("--hold", type=float, default=700)
ap.add_argument("--pointer", action="store_true")
ap.add_argument("--pointer-scale", type=float, default=1.0)
args = ap.parse_args()
d = Path(args.dir)
out = Path(args.out) if args.out else d / "demo.gif"

rows = list(csv.DictReader(open(d / "frames.csv")))
times = [float(r["ms"]) / args.slowdown for r in rows]
crop = tuple(int(v) for v in args.crop.split(",")) if args.crop else None
if args.pointer and rows and "x" not in rows[0]:
    raise SystemExit(f"{d / 'frames.csv'} has no x and y columns for --pointer")

# Windows' arrow, its tip at (0, 0), in pixels at 96 DPI
ARROW = [(0, 0), (0, 16), (4, 12), (7, 19), (10, 18), (7, 11), (12, 11)]


def draw_pointer(img, x, y):
    k = args.pointer_scale * args.scale
    ox, oy = (crop[0], crop[1]) if crop else (0, 0)
    px, py = (x - ox) * args.scale, (y - oy) * args.scale
    ImageDraw.Draw(img).polygon([(px + ax * k, py + ay * k) for ax, ay in ARROW], fill=(255, 255, 255),
                                outline=(0, 0, 0), width=max(1, round(k)))


def load(i):
    img = Image.open(d / f"frame{int(rows[i]['frame']):03d}.png").convert("RGB")
    if crop:
        img = img.crop(crop)
    if args.scale != 1.0:
        img = img.resize((round(img.width * args.scale), round(img.height * args.scale)), Image.LANCZOS)
    if args.pointer and int(rows[i]["x"]) >= 0:
        draw_pointer(img, int(rows[i]["x"]), int(rows[i]["y"]))
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
