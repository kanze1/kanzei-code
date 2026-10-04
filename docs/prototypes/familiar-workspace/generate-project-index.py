"""Read workspace manifests and emit the prototype's inspectable dependency index."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import tomllib

base = Path(__file__).resolve().parent
root = base.parents[2]
workspace = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
members = []
for pattern in workspace['workspace']['members']:
    members.extend(sorted(root.glob(pattern)))
packages = {}
for folder in members:
    manifest = folder / 'Cargo.toml'
    data = tomllib.loads(manifest.read_text(encoding='utf-8'))
    packages[data['package']['name']] = (folder, manifest, data)
edges, nodes, manifests = [], [], []
for name, (folder, manifest, data) in packages.items():
    manifests.append(manifest)
    nodes.append({'id':name,'path':folder.relative_to(root).as_posix(),'manifest':manifest.relative_to(root).as_posix()})
    groups = [(key,data.get(key,{})) for key in ('dependencies','build-dependencies','dev-dependencies')]
    for target, config in data.get('target',{}).items():
        groups.extend((f'{key} · {target}',config.get(key,{})) for key in ('dependencies','build-dependencies','dev-dependencies'))
    for kind, dependencies in groups:
        for alias, settings in dependencies.items():
            if not isinstance(settings,dict):
                continue
            if settings.get('workspace'):
                inherited = workspace.get('workspace',{}).get('dependencies',{}).get(alias,{})
                settings = {**(inherited if isinstance(inherited,dict) else {}),**settings}
            target = settings.get('package',alias)
            if target in packages:
                old = next((e for e in edges if e['from']==name and e['to']==target),None)
                if old:
                    if kind not in old['kinds']: old['kinds'].append(kind)
                else:
                    edges.append({'from':name,'to':target,'kinds':[kind],'source':manifest.relative_to(root).as_posix()})
normal = [e for e in edges if any(not k.startswith('dev-') for k in e['kinds'])]
def level(name, seen=frozenset()):
    if name in seen:
        return 0
    children = [e['to'] for e in normal if e['from']==name]
    return 1 + max((level(n,seen|{name}) for n in children),default=-1)
for node in nodes: node['level']=level(node['id'])
head = subprocess.check_output(['git','rev-parse','--short=8','HEAD'],cwd=root,text=True).strip()
fingerprint = hashlib.sha256(b''.join(p.read_bytes() for p in [root/'Cargo.toml',*manifests])).hexdigest()[:16]
index = {'head':head,'manifestFingerprint':fingerprint,'source':'Cargo.toml + crates/*/Cargo.toml','nodes':nodes,'edges':edges}
app = base/'app.js'
text = app.read_text(encoding='utf-8')
block = '  // BEGIN GENERATED PROJECT INDEX\n  const PROJECT_INDEX = '+json.dumps(index,ensure_ascii=False,separators=(',',':'))+';\n  // END GENERATED PROJECT INDEX'
pattern = r'  // BEGIN GENERATED PROJECT INDEX\n.*?  // END GENERATED PROJECT INDEX'
if re.search(pattern,text,re.S): text = re.sub(pattern,lambda _:block,text,flags=re.S)
else: text = text.replace("  const FEEDBACK_KEY = `${KEY}-feedback`;", "  const FEEDBACK_KEY = `${KEY}-feedback`;\n"+block)
app.write_text(text,encoding='utf-8')
print(json.dumps({'head':head,'modules':len(nodes),'directEdges':len(edges),'manifestFingerprint':fingerprint}))
