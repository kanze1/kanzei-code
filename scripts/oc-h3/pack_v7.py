"""Package visually reviewed, offline-matted whole-character clips."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT/'crates/kanzei-app/ui/assets/oc'
LABELS = {
    'idle-breath':'轻呼吸','idle-observe':'短暂移开视线','idle-quiet':'安静停留',
    'listening':'注视与回应','thinking':'低头思考','replying':'抬手说明与收回',
    'executing':'查看与核对','blocked':'停顿与疑惑','aside':'侧眼吐槽','warm':'会意','complete':'轻点头确认',
}


def read(path): return json.loads(path.read_text(encoding='utf-8'))
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    records={}
    destination=ASSETS/'clips-v7'
    for name,label in LABELS.items():
        job=f'oc-{name}-workwear-v7'+('-soft' if name.startswith('idle-') else '')
        source=ROOT/'output/oc-h3'/job
        review=read(source/'review/visual-review.json')
        checks=read(source/'review/checks.json')
        tracking=read(source/'review/tracking.json')
        bake=read(destination/(name+'.bake.json'))
        if bake.get('closed_mouth')=='clean-plate':
            tracking=read(destination/(name+'.json'))
        movie=destination/(name+'.mp4')
        if review.get('status')!='accepted' or checks['decode']!='passed':
            raise RuntimeError('Unreviewed video: '+name)
        if tracking['frames']!=checks['frames'] or bake['frames']!=checks['frames'] or bake['sha256']!=sha(movie):
            raise RuntimeError('Source/bake mismatch: '+name)
        if bake['size']!=[576,864] or bake['encoded_size']!=[576,1728] or bake['fps']!=24:
            raise RuntimeError('Unexpected packed format: '+name)
        records[name]=(source,checks,tracking,bake,label)
    clips={}
    for name,(source,checks,tracking,bake,label) in records.items():
        track=destination/(name+'.json')
        tracking['size']=[576,864]
        track.write_text(json.dumps(tracking,separators=(',',':')),encoding='utf-8')
        clips[name]=dict(file=f'clips-v7/{name}.mp4',tracking=f'clips-v7/{name}.json',
            sha256=bake['sha256'],trackingSha256=sha(track),frames=checks['frames'],start=0,end=checks['frames']/24,
            label=label,source=dict(job=source.name,sha256=sha(source/'sample.mp4'),
                postproduction=read(source/'postproduction.json') if (source/'postproduction.json').exists() else None))
    # The complete hand gesture returns to neutral within this single clip.
    # Keep every transition out of the active gesture; then allow quiet speech.
    clips['replying'].update(next='reply-rest',protected=[[.55,4.55]])
    for name,bounds in {
        'listening':[.35,3.5],'thinking':[.55,5.5],'executing':[.55,5.5],
        'blocked':[.35,3.5],'aside':[.35,3.5],'warm':[.35,3.5],'complete':[.35,3.5],
    }.items(): clips[name]['protected']=[bounds]
    clips['reply-rest']={**clips['idle-quiet'],'start':5,'protected':[],'next':'replying','label':'平视回应'}
    fixed_details=all(record[3].get('closed_mouth')=='clean-plate' for record in records.values())
    pack=dict(format='kanzei.character-pack.v3',version='2026.09.25.7.1' if fixed_details else '2026.09.24.7',fps=24,size=[576,864],
        videoLayout='rgb-alpha-vertical',poster='master-workwear-v7-alpha.png',posterSha256=sha(ASSETS/'master-workwear-v7-alpha.png'),
        mouth=dict(texture='mouth-soft-v6.png',reference=[.498,.234],tracking='per-frame affine',closed='baked'),
        background=[218,209,203],transitionMs=180,
        aliases=dict(interrupted='idle',stopping='idle',error='blocked'),
        states={
            'idle':dict(clips=['idle-breath','idle-observe','idle-quiet']),
            'listening':dict(clips=['listening','idle-breath','idle-observe']),
            'thinking':dict(clips=['thinking','idle-quiet']),
            'replying':dict(clips=['replying']),
            'executing':dict(clips=['executing','idle-breath']),
            'blocked':dict(clips=['blocked','idle-quiet']),
            'aside':dict(clips=['aside','idle-quiet']),
            'warm':dict(clips=['warm'],once=True),'complete':dict(clips=['complete'],once=True),
        },clips=clips,demo=read(ROOT/'scripts/oc-h3/demo-v7.json'))
    if fixed_details:
        pack['basePoster']='base-workwear-v7-alpha.png'
        pack['basePosterSha256']=sha(ASSETS/pack['basePoster'])
        pack['mouth'].update(closed='clean-plate',closedTexture='detail-reference-v7.png',
            closedTextureSha256=sha(ASSETS/'detail-reference-v7.png'),composite='source ink residual')
        pack['tattoo']=dict(texture='reference.png',sha256=sha(ASSETS/'reference.png'),
            reference=[811,1017],textureSize=[1254,1254],anchor=[.588,.340],scale=.3,
            tracking='per-frame neck affine',composite='source pigment')
    (ASSETS/'character-v7.json').write_text(json.dumps(pack,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(dict(videos=len(records),video_bytes=sum((destination/(name+'.mp4')).stat().st_size for name in records))))


if __name__=='__main__': main()
