"""Make diagnostic crops around mouth anchors, including their largest steps."""
import argparse
import json
from pathlib import Path

import cv2
import numpy as np

parser = argparse.ArgumentParser()
parser.add_argument("movie", type=Path)
args = parser.parse_args()
tracking = json.loads((args.movie.parent/"review/tracking.json").read_text(encoding="utf-8"))
tracks = np.array(tracking["mouth"])
width, height = tracking["size"]
jumps = np.linalg.norm(np.diff(tracks[:, :2], axis=0)*[width, height], axis=1)
selected = set(np.linspace(0, len(tracks)-1, 12).astype(int).tolist())
for i in np.argsort(jumps)[-3:]:
    selected.update([int(i), int(i+1)])
selected = sorted(selected)
sheet = np.full(((len(selected)+5)//6*158, 6*240, 3), 245, np.uint8)
capture = cv2.VideoCapture(str(args.movie))
for cell, index in enumerate(selected):
    capture.set(cv2.CAP_PROP_POS_FRAMES, index)
    ok, frame = capture.read()
    if not ok: raise RuntimeError("Missing review frame")
    x, y = np.round(tracks[index, :2]*[width, height]).astype(int)
    crop = frame[y-34:y+34, x-60:x+60].copy()
    cv2.ellipse(crop, (60,34), (27,16), tracks[index,3]*180/np.pi, 0, 360, (30,190,40), 1)
    crop = cv2.resize(crop, (240,136), interpolation=cv2.INTER_NEAREST)
    top, left = cell//6*158, cell%6*240
    sheet[top:top+136,left:left+240] = crop
    cv2.putText(sheet, f"{index:03} / {index/tracking['fps']:.2f}s", (left+5,top+152),cv2.FONT_HERSHEY_SIMPLEX,.42,(30,30,30),1)
capture.release()
output = args.movie.parent/"review/mouth-tracking-review.png"
cv2.imwrite(str(output),sheet)
print(output)
