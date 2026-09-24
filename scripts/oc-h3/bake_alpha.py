"""Bake complete video frames and closed mouths into RGB/alpha H.264 planes.

The upper plane stores straight RGB, the lower plane stores a greyscale matte.
The runtime only samples these two planes; it does not chroma-key every pixel.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

import cv2
import numpy as np


def smooth(a, b, value):
    t = np.clip((value-a)/(b-a), 0, 1)
    return t*t*(3-2*t)


def closed_mouth(frame, reference, track):
    height, width = frame.shape[:2]
    ax, ay, scale, angle = track[:4]
    cx, cy = ax*width, ay*height
    x0, x1 = max(0, int(cx-width*.06)), min(width, int(cx+width*.06)+1)
    y0, y1 = max(0, int(cy-height*.03)), min(height, int(cy+height*.03)+1)
    yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
    c, s = np.cos(angle), np.sin(angle)
    lx = (c*(xx-cx)+s*(yy-cy))/max(.8, scale)/width
    ly = (-s*(xx-cx)+c*(yy-cy))/max(.8, scale)/height
    region = 1-smooth(.72, 1, np.sqrt((lx/.042)**2+(ly/.018)**2))
    opening = 1-smooth(.82, 1, np.sqrt((lx/.024)**2+(ly/.007)**2))
    patch = cv2.remap(reference, ((.498+lx)*width).astype(np.float32),
                      ((.234+ly)*height).astype(np.float32), cv2.INTER_LINEAR,
                      borderMode=cv2.BORDER_REPLICATE)

    def sample(image, x, y):
        return image[np.clip(round(y*height), 0, height-1), np.clip(round(x*width), 0, width-1)]

    shift = np.clip(sample(frame, ax, ay-.019)-sample(reference, .498, .234-.019), -.12, .12)
    patch = np.clip(patch+shift, 0, 1)
    fraction = np.clip(.5+lx/.080, 0, 1)[..., None]
    skin = sample(frame, ax-.040, ay)*(1-fraction)+sample(frame, ax+.040, ay)*fraction
    patch = skin*(1-opening[..., None])+patch*opening[..., None]
    frame[y0:y1, x0:x1] = frame[y0:y1, x0:x1]*(1-region[..., None])+patch*region[..., None]


def matte(frame):
    height, width = frame.shape[:2]
    # The generated backdrop has broad warm-grey lighting variation. Brightness
    # distance alone mistakes that variation for a halo around skin and hair.
    # Use colour/chroma seeds, then fill the connected figure's interior.
    skin = (frame[...,0]-frame[...,2]>.12)&(frame[...,0]>frame[...,1])&(frame[...,1]>frame[...,2])
    dark = np.max(frame,axis=2)<.64
    blue = frame[...,2]-frame[...,0]>.08
    hard = (dark|skin|blue).astype(np.uint8)
    count, labels, stats, _ = cv2.connectedComponentsWithStats(hard, 8)
    if count < 2:
        raise RuntimeError('No foreground silhouette in video frame')
    largest = 1+np.argmax(stats[1:,cv2.CC_STAT_AREA])
    silhouette = (labels==largest).astype(np.uint8)
    contours,_ = cv2.findContours(silhouette,cv2.RETR_EXTERNAL,cv2.CHAIN_APPROX_SIMPLE)
    silhouette.fill(0)
    cv2.drawContours(silhouette,contours,-1,1,cv2.FILLED)
    neutral = (np.max(frame,axis=2)-np.min(frame,axis=2)<.115)&(np.min(frame,axis=2)>.60)
    holes = (neutral&(silhouette>0)).astype(np.uint8)
    holes[round(height*.10):round(height*.32),round(width*.36):round(width*.64)] = 0
    probable = cv2.dilate(silhouette,np.ones((5,5),np.uint8))
    gc = np.where(probable,cv2.GC_PR_BGD,cv2.GC_BGD).astype(np.uint8)
    gc[silhouette>0] = cv2.GC_PR_FGD
    gc[holes>0] = cv2.GC_PR_BGD
    core = cv2.erode(silhouette,np.ones((5,5),np.uint8))>0
    gc[core&(dark|skin|blue)&~neutral] = cv2.GC_FGD
    cv2.setRNGSeed(7)
    cv2.grabCut(np.uint8(frame*255),gc,None,np.zeros((1,65)),np.zeros((1,65)),2,cv2.GC_INIT_WITH_MASK)
    silhouette = np.isin(gc,[cv2.GC_FGD,cv2.GC_PR_FGD]).astype(np.uint8)
    _, components, areas, _ = cv2.connectedComponentsWithStats(silhouette,8)
    silhouette = (components==1+np.argmax(areas[1:,cv2.CC_STAT_AREA])).astype(np.uint8)
    exterior = 1-cv2.dilate(silhouette,np.ones((7,7),np.uint8))
    _, bg_labels = cv2.distanceTransformWithLabels(1-exterior,cv2.DIST_L2,5,labelType=cv2.DIST_LABEL_PIXEL)
    background_colors = frame[exterior.astype(bool)]
    background = background_colors[np.clip(bg_labels-1,0,len(background_colors)-1)]
    difference = frame-background
    solid = cv2.erode(silhouette,np.ones((3,3),np.uint8))
    edge_distance, nearest_labels = cv2.distanceTransformWithLabels(1-solid,cv2.DIST_L2,5,labelType=cv2.DIST_LABEL_PIXEL)
    foreground_colors = frame[solid.astype(bool)]
    nearest_color = foreground_colors[np.clip(nearest_labels-1,0,len(foreground_colors)-1)]
    direction = nearest_color-background
    estimated = np.clip(np.sum(difference*direction,axis=2)/np.maximum(np.sum(direction*direction,axis=2),.0001),0,1)
    edge_zone = (solid==0)&(edge_distance<2)
    alpha = np.where(edge_zone,estimated,solid.astype(np.float32))
    # Ink outlines are opaque foreground, not mixtures of skin and backdrop.
    alpha = np.maximum(alpha,(dark&(silhouette>0)).astype(np.float32))
    # Foreground seeds define the boundary; keep a single antialiased fringe.
    support = silhouette.astype(np.float32)
    alpha *= support
    alpha *= smooth(.02,.10,alpha)
    alpha = np.clip(alpha, 0, 1)
    foreground = np.clip((frame-(1-alpha[...,None])*background)/np.maximum(alpha[...,None],.001),0,1)
    foreground = np.where((edge_zone&(alpha<.95))[...,None],nearest_color,foreground)
    foreground[alpha<.004] = 0
    return foreground, alpha


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('movie', type=Path)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--tracking', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--poster', type=Path)
    parser.add_argument('--width', type=int, default=576)
    args = parser.parse_args()
    cv2.setNumThreads(2)
    capture = cv2.VideoCapture(str(args.movie))
    fps = capture.get(cv2.CAP_PROP_FPS)
    width = int(capture.get(cv2.CAP_PROP_FRAME_WIDTH))
    height = int(capture.get(cv2.CAP_PROP_FRAME_HEIGHT))
    count = int(capture.get(cv2.CAP_PROP_FRAME_COUNT))
    track = json.loads(args.tracking.read_text(encoding='utf-8'))
    if len(track['mouth']) != count or fps != 24:
        raise RuntimeError('Tracking and video frame counts differ')
    reference = cv2.cvtColor(cv2.imread(str(args.reference)), cv2.COLOR_BGR2RGB)
    reference = cv2.resize(reference,(width,height),interpolation=cv2.INTER_AREA).astype(np.float32)/255
    out_w, out_h = args.width, round(args.width*height/width/2)*2
    args.output.parent.mkdir(parents=True,exist_ok=True)
    encoder = subprocess.Popen(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','rawvideo','-pix_fmt','rgb24',
        '-s',f'{out_w}x{out_h*2}','-r','24','-i','pipe:0','-an','-c:v','libx264','-preset','fast',
        '-crf','17','-pix_fmt','yuv420p','-g','24','-keyint_min','24','-sc_threshold','0','-movflags','+faststart',str(args.output)],stdin=subprocess.PIPE)
    frames = 0
    try:
        while True:
            ok, raw = capture.read()
            if not ok: break
            frame = cv2.cvtColor(raw,cv2.COLOR_BGR2RGB).astype(np.float32)/255
            closed_mouth(frame,reference,track['mouth'][frames])
            rgb, alpha = matte(frame)
            # Resize premultiplied colour to keep dark-theme hair edges clean.
            premult = cv2.resize(rgb*alpha[...,None],(out_w,out_h),interpolation=cv2.INTER_AREA)
            alpha = cv2.resize(alpha,(out_w,out_h),interpolation=cv2.INTER_AREA)
            rgb = np.clip(premult/np.maximum(alpha[...,None],.001),0,1)
            color = np.uint8(np.rint(rgb*255))
            mask = np.uint8(np.rint(alpha*255))
            if frames==0 and args.poster:
                args.poster.parent.mkdir(parents=True,exist_ok=True)
                cv2.imwrite(str(args.poster),cv2.cvtColor(np.dstack([color,mask]),cv2.COLOR_RGBA2BGRA))
            encoder.stdin.write(np.vstack([color,np.repeat(mask[...,None],3,axis=2)]).tobytes())
            frames += 1
    finally:
        capture.release(); encoder.stdin.close()
        code = encoder.wait()
    if code or frames != count: raise RuntimeError(f'Incomplete packed video: {frames}/{count}, encoder={code}')
    record = dict(format='rgb-alpha-vertical', source=str(args.movie), frames=frames, fps=fps,
        size=[out_w,out_h], encoded_size=[out_w,out_h*2], bytes=args.output.stat().st_size,
        sha256=hashlib.sha256(args.output.read_bytes()).hexdigest(), closed_mouth='baked', keying='offline',
        matte='graph-cut-with-ink-preservation',opencv=cv2.__version__,
        pipeline_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    args.output.with_suffix('.bake.json').write_text(json.dumps(record,indent=2),encoding='utf-8')
    print(json.dumps(record))


if __name__ == '__main__':
    main()
