"""Record the approved C voice through the configured local speech gateway."""
import hashlib
import json
from pathlib import Path
import urllib.request
import wave

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / 'crates/kanzei-app/ui/assets/oc'
OUT = ROOT / 'output/oc-v7/voice'


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen('http://127.0.0.1:7389/health',timeout=5) as response:
        health = json.load(response)
    if not health.get('ttsReady') or health.get('voice') != 'Kanzei OC CN C':
        raise RuntimeError('The selected C voice is not ready')
    rate = health['sampleRate']
    audio = bytearray(30*rate*2)
    cues = []
    for at,state,text,limit in [
        (3.1,'listening','说吧，今天折腾什么。',3.3),
        (15.7,'replying','找到毛病了。这个地方接错了，改一下就好。',5.5),
        (22.3,'aside','这弯绕得，还挺有创意。',3.8),
        (26.8,'complete','好了，试试。',3.2),
    ]:
        path = OUT / (state+'.wav')
        if path.exists():
            with wave.open(str(path),'rb') as wav:
                if wav.getframerate()!=rate: raise RuntimeError('Unexpected cached sample rate')
                pcm = wav.readframes(wav.getnframes())
        else:
            request = urllib.request.Request('http://127.0.0.1:7389/speak',
                data=json.dumps(dict(text=text,language='zh'),ensure_ascii=False).encode(),
                headers={'Content-Type':'application/json'})
            with urllib.request.urlopen(request,timeout=90) as response: pcm=response.read()
            with wave.open(str(path),'wb') as wav:
                wav.setparams((1,2,rate,0,'NONE','not compressed'));wav.writeframes(pcm)
        seconds=len(pcm)/(rate*2)
        if seconds>limit or len(pcm)%2: raise RuntimeError(f'Cue exceeds its window: {state}, {seconds}')
        offset=round(at*rate)*2
        audio[offset:offset+len(pcm)]=pcm
        cues.append(dict(at=at,state=state,text=text,duration=seconds))
    target=ASSETS/'demo-speech-c-v7.wav'
    with wave.open(str(target),'wb') as wav:
        wav.setparams((1,2,rate,0,'NONE','not compressed'));wav.writeframes(audio)
    demo=dict(audio=target.name,duration=30,fps=24,voice=health['voice'],referenceSha256=health['referenceSha256'],
        audioSha256=hashlib.sha256(target.read_bytes()).hexdigest(),
        events=[[0,'idle'],[2.4,'listening'],[6.5,'thinking'],[13.2,'replying'],[21.5,'aside'],[26.2,'complete'],[29.6,'idle']],cues=cues)
    (ROOT/'scripts/oc-h3/demo-v7.json').write_text(json.dumps(demo,ensure_ascii=False,indent=2),encoding='utf-8')
    (OUT/'health.json').write_text(json.dumps(health,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(demo,ensure_ascii=False))


if __name__=='__main__': main()
