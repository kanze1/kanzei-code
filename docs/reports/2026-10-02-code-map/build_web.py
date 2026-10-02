"""Build the offline audit viewer from the report and source inventory."""
from pathlib import Path
import hashlib
import html
import json
import re
from datetime import datetime, timezone

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[2]
WEB = OUT / 'web'
inventory = json.loads((OUT / 'inventory.json').read_text(encoding='utf-8'))
findings = json.loads((WEB / 'findings.json').read_text(encoding='utf-8'))
curation = json.loads((WEB / 'curation.json').read_text(encoding='utf-8'))
handled_ids = set(curation['handled_issue_ids'])
excluded_ids = set(curation['excluded_issue_ids'])
findings = [f for f in findings if f['id'] not in excluded_ids]
for finding in findings:
    finding['handled'] = finding['id'] in handled_ids

def plain(text):
    text = re.sub(r'\[([^\]]+)\]\([^\n]+?\)', r'\1', text)
    return text.replace('`', '').replace('\\|', '|')

def rows(text):
    group = ''
    for line in text.splitlines():
        if line.startswith('## '):
            group = line[3:].strip()
        elif line.startswith('|') and not re.match(r'^\|[\s:-]+\|', line):
            parts = re.split(r'(?<!\\)\|', line.strip()[1:-1])
            yield group, [part.strip() for part in parts]

def inline(text):
    escaped = html.escape(text)
    escaped = re.sub(r'`([^`]+)`', r'<code>\1</code>', escaped)
    escaped = re.sub(r'\*\*([^*]+)\*\*', r'<strong>\1</strong>', escaped)
    # Render source references as text; source buttons in the inspector are actionable.
    escaped = re.sub(r'\[([^\]]+)\]\([^\n]+?\)', r'<span class="reference">\1</span>', escaped)
    return escaped

def markdown(text):
    result, paragraph, table = [], [], []
    fenced = None
    fence = []
    def flush():
        if paragraph:
            result.append('<p>' + inline(' '.join(paragraph)) + '</p>')
            paragraph.clear()
        if table:
            headers = table[0]
            body = table[1:]
            result.append('<div class="doc-table"><table><thead><tr>' + ''.join('<th>'+inline(x)+'</th>' for x in headers) + '</tr></thead><tbody>' + ''.join('<tr>' + ''.join('<td>'+inline(x)+'</td>' for x in row) + '</tr>' for row in body) + '</tbody></table></div>')
            table.clear()
    for line in text.splitlines():
        if line.startswith('```'):
            if fenced is None:
                flush()
                fenced = line[3:].strip()
            else:
                result.append('<pre class="document-code"><code>' + html.escape('\n'.join(fence)) + '</code></pre>')
                fenced, fence = None, []
            continue
        if fenced is not None:
            fence.append(line)
        elif line.startswith('#') and (m := re.match(r'^(#{1,4})\s+(.+)', line)):
            flush()
            level = min(len(m.group(1)) + 1, 5)
            result.append(f'<h{level}>' + inline(m.group(2)) + f'</h{level}>')
        elif line.startswith('|'):
            if paragraph:
                flush()
            if not re.match(r'^\|[\s:-]+\|', line):
                table.append([x.strip() for x in re.split(r'(?<!\\)\|', line.strip()[1:-1])])
        elif not line.strip():
            flush()
        else:
            if table:
                flush()
            paragraph.append(line.strip())
    flush()
    return ''.join(result)

files = inventory['files']
paths = {f['path']: f for f in files}
source_evidence = {}
for finding in findings:
    finding['evidence'] = []
    for path, line in finding['refs']:
        source = ROOT / path
        if not source.is_file():
            raise ValueError(f'Missing source: {path}')
        raw = source.read_bytes()
        lines = raw.decode('utf-8-sig', errors='replace').splitlines()
        if not 1 <= line <= len(lines):
            raise ValueError(f'Invalid evidence line: {path}:{line}')
        key = f'{path}:{line}'
        source_evidence[key] = {'path': path, 'line': line, 'start': max(1, line-7), 'text': '\n'.join(lines[max(0,line-8):line+12]), 'sha256': hashlib.sha256(raw).hexdigest()}
        finding['evidence'].append(key)

feature_text = (OUT / '01-features.md').read_text(encoding='utf-8')
features = []
for group, cells in rows(feature_text):
    if len(cells) != 3 or cells[0] == '功能':
        continue
    name, entry, logic = map(plain, cells)
    refs = []
    for token in re.findall(r'[\w/.-]+\.(?:rs|js|mjs)', entry+' '+logic):
        matches = [f['path'] for f in files if f['path'].endswith('/'+token) or f['path'].endswith('/'+token.replace('app/', '').replace('core/', ''))]
        refs += matches[:4]
    features.append({'id': f'F{len(features)+1:03}', 'title': name, 'domain': group, 'entry': entry, 'implementation': logic, 'paths': list(dict.fromkeys(refs)), 'issues': [f['id'] for f in findings if not f['handled'] and name in f['features']]})

feature_by_id = {f['id']: f for f in features}
candidates = []
for spec in curation['candidates']:
    members = [feature_by_id[member] for member in spec['members']]
    candidates.append({**spec, 'member_names': [m['title'] for m in members], 'entry': '；'.join(m['entry'] for m in members), 'implementation': '\n\n'.join(m['title'] + '：' + m['implementation'] for m in members), 'paths': list(dict.fromkeys(p for m in members for p in m['paths'])), 'issues': []})

decision_text = (ROOT / '.kanzei/project/decisions.md').read_text(encoding='utf-8-sig')
original_decisions = {}
for match in re.finditer(r'^## (A-\d+) (.*)\n([\s\S]*?)(?=^## |\Z)', decision_text, re.M):
    original_decisions[match[1]] = {'title': match[2], 'body': match[3].strip(), 'line': decision_text.count('\n',0,match.start())+1}
decisions = []
for group, cells in rows((OUT / '07-cleanup-decisions.md').read_text(encoding='utf-8')):
    if group != '全部现存 A 决策逐项对照' or len(cells) != 3:
        continue
    m = re.match(r'(A-\d+) (.+)', cells[0])
    if m:
        original = original_decisions[m[1]]
        decisions.append({'id': m[1], 'title': plain(m[2]), 'status': cells[1], 'implementation': plain(cells[2]), 'original': original['body'], 'line': original['line'], 'issues': [f['id'] for f in findings if not f['handled'] and m[1] in f['decisions']]})

docs = []
for name in ['02-runtime.md', '03-work-decisions.md', '04-memory-storage.md', '05-tools-research.md', '06-desktop-delivery.md', '07-cleanup-decisions.md']:
    text = (OUT / name).read_text(encoding='utf-8')
    for part in re.split(r'(?=^## )', text, flags=re.M):
        if not part.startswith('## '):
            continue
        title = part.splitlines()[0][3:]
        docs.append({'id': f'L{len(docs)+1:03}', 'title': title, 'domain': name[3:-3], 'text': plain(part), 'html': markdown(part), 'report': name})

payload = {'version':2, 'generatedAt':datetime.now(timezone.utc).isoformat(), 'snapshotAt':inventory['captured_at_utc'], 'head':inventory['head'], 'branch':inventory['branch'], 'findings':findings, 'features':features, 'candidates':candidates, 'curation':curation, 'decisions':decisions, 'documents':docs, 'files':[{'path':f['path'], 'domain':f['domain'], 'lines':f['lines'], 'status':f['status'], 'header':' '.join(f['header']), 'sha256':f['sha256'], 'symbols':f['symbols']} for f in files], 'evidence':source_evidence}
(WEB / 'data.js').write_text('window.KANZEI_AUDIT = ' + json.dumps(payload, ensure_ascii=False, separators=(',',':')).replace('</','<\\/') + ';\n', encoding='utf-8')
print(json.dumps({'archived_findings':len(findings),'candidates':len(candidates),'reference_features':len(features),'decisions':len(decisions),'logic_sections':len(docs),'files':len(files),'source_evidence':len(source_evidence)}, ensure_ascii=False))
