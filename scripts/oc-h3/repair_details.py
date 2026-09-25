"""Prepare clean video plates and track the original character's fixed details.

The existing full-body RGB/alpha video is retained. Only mouth/neck repair
regions are replaced from the original motion frames; crisp ink is composited
from the source illustrations by the player, outside the video codec.
"""
import argparse
import hashlib
import json
import math
import subprocess
from pathlib import Path

import cv2
import numpy as np


def feather(value, a=.78, b=1):
    t = np.clip((value-a)/(b-a), 0, 1)
    return 1-t*t*(3-2*t)


def neck_tracks(frames):
    height, width = frames[0].shape[:2]
    first = cv2.cvtColor(frames[0], cv2.COLOR_BGR2GRAY)
    mask = np.zeros_like(first)
    mask[round(height*.30):round(height*.405), round(width*.48):round(width*.66)] = 255
    points = cv2.goodFeaturesToTrack(first, 90, .015, 4, mask=mask)
    if points is None or len(points)<6:
        raise RuntimeError('Not enough neck landmarks')
    reference = np.array([width*.588, height*.340, 1.])
    result = []
    last = [.588, .340, 1., 0., 1.]
    for frame in frames:
        grey = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
        forward, ok, _ = cv2.calcOpticalFlowPyrLK(first, grey, points, None, winSize=(25,25), maxLevel=3)
        back, valid, _ = cv2.calcOpticalFlowPyrLK(grey, first, forward, None, winSize=(25,25), maxLevel=3)
        keep = ok.ravel().astype(bool)&valid.ravel().astype(bool)&(np.linalg.norm((back-points).reshape(-1,2),axis=1)<.6)
        confidence = 0.
        if keep.sum()>=6:
            transform, inliers = cv2.estimateAffinePartial2D(points[keep],forward[keep],method=cv2.RANSAC,ransacReprojThreshold=.8)
            if transform is not None:
                scale=math.hypot(transform[0,0],transform[1,0])
                angle=math.atan2(transform[1,0],transform[0,0])
                if .88<scale<1.12 and abs(angle)<.20:
                    center=transform@reference
                    confidence=float(inliers.mean())
                    last=[center[0]/width,center[1]/height,scale,angle,confidence]
        result.append([*last[:4],confidence])
    array=np.array(result)
    array[:,:4]=cv2.GaussianBlur(array[:,:4],(1,5),0,sigmaY=.7)
    return np.round(array,6).tolist()


def repair_region(frame, track, kind):
    height,width=frame.shape[:2]
    ax,ay,scale,angle=track[:4]
    cx,cy=ax*width,ay*height
    half=(.061,.032) if kind=='mouth' else (.038,.049)
    x0,x1=max(0,int(cx-half[0]*width)),min(width,int(cx+half[0]*width)+1)
    y0,y1=max(0,int(cy-half[1]*height)),min(height,int(cy+half[1]*height)+1)
    yy,xx=np.mgrid[y0:y1,x0:x1].astype(np.float32)
    cosine,sine=np.cos(angle),np.sin(angle)
    lx=(cosine*(xx-cx)+sine*(yy-cy))/scale/width
    ly=(-sine*(xx-cx)+cosine*(yy-cy))/scale/height
    local=frame[y0:y1,x0:x1]
    if kind=='mouth':
        # Remove the complete authored lip line; its high-resolution source is
        # sampled in the player for closed and open shapes alike.
        erase=((lx/.027)**2+(ly/.012)**2<1).astype(np.uint8)*255
        area=feather(np.sqrt((lx/.052)**2+(ly/.025)**2))
    else:
        # Include antialiased brown/grey fragments, not just the darkest ink.
        # The source tattoo occupies this narrow tracked strip of exposed skin.
        bounds=(np.abs(lx)<.023)&(ly>-.039)&(ly<.043)
        erase=cv2.dilate(bounds.astype(np.uint8),np.ones((3,3),np.uint8))*255
        area=feather(np.maximum(np.abs(lx)/.035,np.abs(ly)/.047))
    if kind=='mouth':
        clean=cv2.inpaint(local,erase,4,cv2.INPAINT_TELEA)
    else:
        # Reconstruct skin from skin samples only. Cloth beside the neckline
        # must not bleed into the cleared tattoo strip through inpainting.
        rgb=local[...,::-1].astype(np.float64)/255
        warm=((rgb[...,0]-rgb[...,2]>.13)&(rgb[...,0]>rgb[...,1])&(rgb.mean(axis=2)>.63)).astype(np.uint8)
        contours,_=cv2.findContours(warm,cv2.RETR_EXTERNAL,cv2.CHAIN_APPROX_SIMPLE)
        exposed=np.zeros_like(warm)
        if contours:cv2.drawContours(exposed,[max(contours,key=cv2.contourArea)],-1,1,cv2.FILLED)
        erase*=exposed
        skin=(rgb[...,0]-rgb[...,2]>.15)&(rgb[...,0]>rgb[...,1])&(rgb.mean(axis=2)>.70)&(erase==0)
        basis=np.stack([np.ones_like(lx),lx/.04,ly/.05],axis=-1).astype(np.float64)
        if skin.sum()<30:raise RuntimeError('Insufficient exposed skin around tattoo')
        design=basis[skin];observed=local[skin].astype(np.float64)
        weights=np.ones(len(design))
        for _ in range(4):
            solution=np.linalg.lstsq(design*weights[:,None],observed*weights[:,None],rcond=None)[0]
            error=np.linalg.norm(observed-design@solution,axis=1)
            weights=np.minimum(1,6/np.maximum(error,.1))
        surface=np.clip(basis@solution,0,255)
        blend=(cv2.GaussianBlur((erase>0).astype(np.float32),(3,3),.55)*exposed)[...,None]
        clean=np.uint8(np.rint(local*(1-blend)+surface*blend))
    return (x0,y0,x1,y1),clean,area


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('raw',type=Path)
    parser.add_argument('--packed',type=Path,required=True)
    parser.add_argument('--tracking',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--poster',type=Path)
    args=parser.parse_args()
    cv2.setNumThreads(2)
    capture=cv2.VideoCapture(str(args.raw));frames=[]
    while True:
        ok,frame=capture.read()
        if not ok:break
        frames.append(frame)
    capture.release()
    tracking=json.loads(args.tracking.read_text(encoding='utf-8'))
    if len(frames)!=len(tracking['mouth']):raise RuntimeError('Frame/track mismatch')
    tracking['tattoo']=neck_tracks(frames)
    tracking['tattooMethod']='first-frame neck landmarks, forward/backward flow, robust affine'
    encoded=cv2.VideoCapture(str(args.packed))
    width=int(encoded.get(cv2.CAP_PROP_FRAME_WIDTH));height=int(encoded.get(cv2.CAP_PROP_FRAME_HEIGHT))//2
    args.output.parent.mkdir(parents=True,exist_ok=True)
    writer=subprocess.Popen(['ffmpeg','-v','error','-y','-f','rawvideo','-pix_fmt','bgr24','-s',f'{width}x{height*2}',
        '-r','24','-i','pipe:0','-an','-c:v','libx264','-preset','fast','-crf','17','-pix_fmt','yuv420p',
        '-g','24','-keyint_min','24','-sc_threshold','0','-movflags','+faststart',str(args.output)],stdin=subprocess.PIPE)
    try:
        for index,raw in enumerate(frames):
            ok,packed=encoded.read()
            if not ok:raise RuntimeError('Packed video ended early')
            colour=packed[:height].astype(np.float32)
            raw_h,raw_w=raw.shape[:2]
            for kind in ['mouth','tattoo']:
                bounds,clean,area=repair_region(raw,tracking[kind][index],kind)
                x0,y0,x1,y1=bounds
                dest=(round(x0*width/raw_w),round(y0*height/raw_h),round(x1*width/raw_w),round(y1*height/raw_h))
                dx0,dy0,dx1,dy1=dest
                size=(dx1-dx0,dy1-dy0)
                patch=cv2.resize(clean,size,interpolation=cv2.INTER_AREA)
                weight=cv2.resize(area,size,interpolation=cv2.INTER_AREA)[...,None]
                colour[dy0:dy1,dx0:dx1]=colour[dy0:dy1,dx0:dx1]*(1-weight)+patch*weight
            packed[:height]=np.uint8(np.clip(np.rint(colour),0,255))
            if index==0 and args.poster:
                alpha=cv2.cvtColor(packed[height:],cv2.COLOR_BGR2GRAY)
                cv2.imwrite(str(args.poster),np.dstack([packed[:height],alpha]))
            writer.stdin.write(packed.tobytes())
    finally:
        encoded.release();writer.stdin.close();code=writer.wait()
    if code:raise RuntimeError('Encoder failed')
    tracking['size']=[width,height]
    args.output.with_suffix('.json').write_text(json.dumps(tracking,separators=(',',':')),encoding='utf-8')
    sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    record=dict(format='rgb-alpha-vertical',frames=len(frames),fps=24,size=[width,height],encoded_size=[width,height*2],
        bytes=args.output.stat().st_size,sha256=sha(args.output),closed_mouth='clean-plate',tattoo='clean-plate',
        source_sha256=sha(args.packed),raw_sha256=sha(args.raw),pipeline_sha256=sha(Path(__file__)),
        tattoo_track_min_confidence=min(row[4] for row in tracking['tattoo']))
    args.output.with_suffix('.bake.json').write_text(json.dumps(record,indent=2),encoding='utf-8')
    print(json.dumps(record))


if __name__=='__main__':main()
