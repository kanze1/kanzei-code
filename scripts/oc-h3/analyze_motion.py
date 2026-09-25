"""Read video frames to record mouth anchors and regional motion for review."""
import argparse
import json
import math
from pathlib import Path

import cv2
import numpy as np


def mouth_stroke(grey, center):
    """Find the short drawn mouth line near a face-motion prediction."""
    height, width = grey.shape
    cx, cy = center
    x1, x2 = max(0, round(cx)-34), min(width, round(cx)+35)
    y1, y2 = max(0, round(cy)-22), min(height, round(cy)+23)
    local = grey[y1:y2, x1:x2].astype(np.float32)
    ridge = np.maximum(0, cv2.GaussianBlur(local, (0, 0), 2)-local-3)
    binary = cv2.morphologyEx((ridge>2).astype(np.uint8), cv2.MORPH_CLOSE, np.ones((3,13),np.uint8))
    count, labels, stats, _ = cv2.connectedComponentsWithStats(binary, 8)
    candidates = []
    for label in range(1, count):
        x, y, w, h, area = stats[label]
        if x <= 1 or y <= 1 or x+w >= local.shape[1]-1 or y+h >= local.shape[0]-1:
            continue
        if not (5 <= w <= 44 and 1 <= h <= 13 and w >= h*1.5 and area >= 5):
            continue
        weights = ridge*(labels == label)
        if weights.sum() < 20: continue
        point = np.array([x1+x+(w-1)/2, y1+y+(h-1)/2])
        score = w / (1 + .02*np.linalg.norm((point-center)/[1, .6])**2)
        candidates.append((score, point))
    return max(candidates, key=lambda item:item[0])[1] if candidates else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("movie", type=Path)
    args = parser.parse_args()
    capture = cv2.VideoCapture(str(args.movie))
    fps = capture.get(cv2.CAP_PROP_FPS)
    ok, first = capture.read()
    if not ok:
        raise RuntimeError("Cannot read the first video frame")
    height, width = first.shape[:2]
    grey0 = cv2.cvtColor(first, cv2.COLOR_BGR2GRAY)
    anchor_x, anchor_y = round(width * .498), round(height * .234)
    reference = np.array([anchor_x, anchor_y], dtype=float)
    half_w, half_h = round(width * .026), round(height * .009)
    template = grey0[anchor_y-half_h:anchor_y+half_h+1, anchor_x-half_w:anchor_x+half_w+1]
    first_stroke = mouth_stroke(grey0, reference)
    if first_stroke is None:
        raise RuntimeError("Cannot locate the reference mouth stroke")
    mouth_offset = reference-first_stroke
    regions = {
        "head": (.31, .04, .72, .27),
        "collar": (.29, .32, .74, .44),
        "lower_ribs": (.30, .49, .72, .66),
        "hips": (.30, .82, .71, .95),
    }
    points = {}
    for name, (x1, y1, x2, y2) in regions.items():
        mask = np.zeros_like(grey0)
        mask[int(y1 * height):int(y2 * height), int(x1 * width):int(x2 * width)] = 255
        points[name] = cv2.goodFeaturesToTrack(grey0, 50, .02, 8, mask=mask)
    face_mask = np.zeros_like(grey0)
    face_mask[int(.17*height):int(.267*height), int(.40*width):int(.60*width)] = 255
    face_points = cv2.goodFeaturesToTrack(grey0, 60, .01, 5, mask=face_mask)
    mouth = []
    motion = {name: [] for name in regions}
    previous = [.498, .234, 1.0, 0.0, 0.0]
    frame = first
    while frame is not None:
        grey = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
        predicted = np.array(previous[:2])*[width, height]
        rotation = np.eye(2)
        if face_points is not None:
            target, status, _ = cv2.calcOpticalFlowPyrLK(grey0, grey, face_points, None, winSize=(21, 21), maxLevel=3)
            back, reverse_status, _ = cv2.calcOpticalFlowPyrLK(grey, grey0, target, None, winSize=(21, 21), maxLevel=3)
            valid = status.ravel().astype(bool) & reverse_status.ravel().astype(bool) & (np.linalg.norm((back-face_points).reshape(-1, 2), axis=1) < .75)
            if valid.sum() >= 5:
                transform, _ = cv2.estimateAffinePartial2D(face_points[valid], target[valid], method=cv2.RANSAC, ransacReprojThreshold=1)
                if transform is not None:
                    scale = math.hypot(transform[0, 0], transform[1, 0])
                    angle = math.atan2(transform[1, 0], transform[0, 0])
                    if .9 <= scale <= 1.1 and abs(angle) <= .2:
                        previous[2:4] = [scale, angle]
                        predicted = transform @ np.array([anchor_x, anchor_y, 1])
                        rotation = transform[:, :2]
        stroke = mouth_stroke(grey, predicted)
        x1, x2 = max(0,anchor_x-half_w-32), min(width,anchor_x+half_w+33)
        y1, y2 = max(0,anchor_y-half_h-48), min(height,anchor_y+half_h+49)
        scores = cv2.matchTemplate(grey[y1:y2,x1:x2],template,cv2.TM_CCOEFF_NORMED)
        _, confidence, _, location = cv2.minMaxLoc(scores)
        if confidence >= .60:
            px, py = location
            center = np.array([x1+px+half_w,y1+py+half_h],dtype=float)
            for axis in (0,1):
                if (axis == 0 and 0 < px < scores.shape[1]-1) or (axis == 1 and 0 < py < scores.shape[0]-1):
                    before = scores[py,px-1] if axis == 0 else scores[py-1,px]
                    after = scores[py,px+1] if axis == 0 else scores[py+1,px]
                    curvature = float(before-2*scores[py,px]+after)
                    center[axis] += np.clip(.5*float(before-after)/curvature,-.5,.5) if abs(curvature)>1e-6 else 0
            previous[:2] = (center/[width,height]).tolist()
            previous[4] = 1.0
        elif stroke is not None:
            center = stroke + rotation @ mouth_offset
            previous[:2] = (center/[width, height]).tolist()
            previous[4] = 1.0
        else:
            previous[:2] = (predicted/[width, height]).tolist()
            previous[4] = 0.0
        mouth.append(previous.copy())
        for name, source in points.items():
            if source is None:
                motion[name].append([0.0, 0.0])
                continue
            target, status, _ = cv2.calcOpticalFlowPyrLK(grey0, grey, source, None,
                winSize=(21, 21), maxLevel=3,
                criteria=(cv2.TERM_CRITERIA_EPS | cv2.TERM_CRITERIA_COUNT, 40, .005))
            backward, back_status, _ = cv2.calcOpticalFlowPyrLK(grey, grey0, target, None,
                winSize=(21, 21), maxLevel=3)
            consistency = np.linalg.norm((backward-source).reshape(-1, 2), axis=1)
            valid = status.ravel().astype(bool) & back_status.ravel().astype(bool) & (consistency < .75)
            displacement = (target - source).reshape(-1, 2)[valid]
            motion[name].append(np.median(displacement, axis=0).tolist() if len(displacement) else [0.0, 0.0])
        ok, frame = capture.read()
        if not ok:
            frame = None
    capture.release()
    raw = np.array(mouth, dtype=np.float64)
    # Three-frame median removes detection jitter without hiding missing anchors.
    smooth = raw.copy()
    for i in range(len(raw)):
        smooth[i, :4] = np.median(raw[max(0, i - 1):i + 2, :4], axis=0)
    smooth[:, :4] = cv2.GaussianBlur(smooth[:, :4],(1,5),0,sigmaY=.8)
    smooth[0,:4],smooth[-1,:4] = raw[0,:4],raw[-1,:4]
    tracks = {"fps": fps, "size": [width, height], "frames": len(mouth),
              "method": "mouth template, face affine and local stroke fallback",
              "mouth": np.round(smooth, 6).tolist(), "reference_angle": float(np.median(smooth[:3, 3])),
              "reference_scale": float(np.median(smooth[:3, 2]))}
    output = args.movie.parent / "review"
    output.mkdir(exist_ok=True)
    (output / "tracking.json").write_text(json.dumps(tracks, separators=(",", ":")), encoding="utf-8")
    measurements = {}
    for name, values in motion.items():
        data = np.array(values)
        measurements[name] = {"peak_to_peak_px": np.round(np.ptp(data, axis=0), 3).tolist(),
                              "displacement_px": np.round(data, 3).tolist()}
    hips = np.array(motion["hips"])
    relative = {name: np.round(np.ptp(np.array(motion[name]) - hips, axis=0), 3).tolist()
                for name in ("head", "collar", "lower_ribs")}
    result = {"frames": len(mouth), "fps": fps, "mouth_detection_fraction": round(float((raw[:, 4] >= .60).mean()), 4),
              "mouth_max_step_px": round(float(np.linalg.norm(np.diff(smooth[:, :2], axis=0) * [width, height], axis=1).max()), 3),
              "regional_peak_to_peak_px": {k: v["peak_to_peak_px"] for k, v in measurements.items()},
              "relative_to_hips_peak_to_peak_px": relative,
              "note": "Optical-flow measurements support review; they do not certify natural breathing."}
    (output / "motion-analysis.json").write_text(json.dumps({**result, "regions": measurements}, indent=2), encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
