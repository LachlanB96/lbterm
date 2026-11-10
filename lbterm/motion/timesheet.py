"""A contact sheet of a capture.ps1 folder's frames nearest to given times, cropped and zoomed, each
labelled with its time: the times are of real speed (the frames' times divided by SLOWDOWN, the
slowdown they were taken at), so sheets at slowdown 1 and 5 pick the same moments. sheet.py's
--tiles picks frames evenly over the whole run instead.

  python lbterm/motion/timesheet.py DIR OUT X Y W H ZOOM SLOWDOWN T1,T2,... [COLS]
  python lbterm/motion/timesheet.py target/motion/bump-5x flare-corner.png 250 80 70 70 4 5 0,30,60,90,120,160,200,250,300,350,400,500
"""
import csv, sys
from pathlib import Path
from PIL import Image, ImageDraw

d, out = Path(sys.argv[1]), Path(sys.argv[2])
x, y, w, h, zoom = (int(v) for v in sys.argv[3:8])
slow = float(sys.argv[8])
want = [float(t) for t in sys.argv[9].split(',')]
cols = int(sys.argv[10]) if len(sys.argv) > 10 else 6
rows = list(csv.DictReader(open(d / 'frames.csv')))
times = [float(r['ms']) / slow for r in rows]
pick = []
for t in want:
    i = min(range(len(times)), key=lambda k: abs(times[k] - t))
    pick.append(i)
tw, th = w * zoom, h * zoom
sheet = Image.new('RGB', (cols * tw, -(-len(pick) // cols) * (th + 16)), (40, 40, 40))
draw = ImageDraw.Draw(sheet)
for n, i in enumerate(pick):
    img = Image.open(d / f'frame{i:03d}.png').convert('RGB').crop((x, y, x + w, y + h)).resize((tw, th), Image.NEAREST)
    px, py = (n % cols) * tw, (n // cols) * (th + 16)
    sheet.paste(img, (px, py + 16))
    draw.text((px + 4, py + 2), f'{times[i]:.0f} ms after the key', fill=(230, 230, 230))
sheet.save(out)
print(out)
