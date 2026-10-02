"""Validate the report against current source files without running the application."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[2]
data = json.loads((OUT / 'inventory.json').read_text(encoding='utf-8'))
errors = []
line_counts = {}
for f in data['files']:
    path = ROOT / f['path']
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != f['sha256']:
        errors.append(f"Source changed after scan: {f['path']}")
    count = len(raw.decode('utf-8-sig', errors='replace').splitlines())
    line_counts[str(path.resolve())] = count
    if count != f['lines']:
        errors.append(f"Source line count changed: {f['path']}")
for d in data['documents']:
    if hashlib.sha256((ROOT / d['path']).read_bytes()).hexdigest() != d['sha256']:
        errors.append(f"Document changed after scan: {d['path']}")

links_checked = 0
for report in sorted(OUT.glob('*.md')):
    text = report.read_text(encoding='utf-8')
    if '\ufffd' in text:
        errors.append(f"Replacement characters: {report.name}")
    if len(re.findall(r'^```', text, re.M)) % 2:
        errors.append(f"Unclosed fenced block: {report.name}")
    for match in re.finditer(r'(?<!\\)\[[^\]\n]+\]\((?:<([^>\n]+)>|([^\)\n]+))\)', text):
        target = match.group(1) or match.group(2)
        if target.startswith(('https://', 'http://', '#')):
            continue
        with_line = re.fullmatch(r'(.+):(\d+)', target)
        file_target = with_line.group(1) if with_line else target
        path = Path(file_target)
        if not path.is_absolute():
            path = report.parent / path
        if not path.is_file():
            errors.append(f"Missing link target in {report.name}: {target}")
            continue
        if with_line:
            key = str(path.resolve())
            if key not in line_counts:
                line_counts[key] = len(path.read_text(encoding='utf-8-sig', errors='replace').splitlines())
            if not 1 <= int(with_line.group(2)) <= line_counts[key]:
                errors.append(f"Link line out of range in {report.name}: {target}")
        links_checked += 1

registered = {x.split('::')[-1] for x in data['tauri_registered']}
declared = {x['name'] for x in data['tauri_commands']}
if registered != declared:
    errors.append(f"Tauri declaration/registration difference: {sorted(registered ^ declared)}")
known = {x['command_name'] for x in data['tauri_commands']}
unknown = sorted({x['name'] for x in data['ui_literal_invokes']} - known)
decisions = (ROOT / '.kanzei/project/decisions.md').read_text(encoding='utf-8-sig')
decision_ids = set(re.findall(r'^## (A-\d+)', decisions, re.M))
crosswalk = (OUT / '07-cleanup-decisions.md').read_text(encoding='utf-8')
mapped_ids = set(re.findall(r'^\| (A-\d+) ', crosswalk, re.M))
if decision_ids != mapped_ids:
    errors.append(f"Decision crosswalk difference: {sorted(decision_ids ^ mapped_ids)}")
readme = (OUT / 'README.md').read_text(encoding='utf-8')
metrics = {
    'source_files': len(data['files']),
    'source_lines': sum(f['lines'] for f in data['files']),
    'symbol_declarations': sum(len(f['symbols']) for f in data['files']),
    'uncommitted_source_files': sum(f['status'] != 'HEAD' for f in data['files']),
    'tauri_commands': len(data['tauri_commands']),
    'sql_table_names': len({t['name'] for t in data['sql_table_declarations']}),
    'existing_documents': len(data['documents']),
    'resource_paths': len(data['resource_files']),
}
for key, value in metrics.items():
    if str(value) not in readme:
        errors.append(f"Overview metric absent: {key}={value}")
for name in ['scan.py', 'validate.py']:
    compile((OUT / name).read_text(encoding='utf-8'), name, 'exec')
head = subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']).decode().strip()
if head != data['head']:
    errors.append('HEAD changed after scan')
receipt = {
    'checked_at_utc': datetime.now(timezone.utc).isoformat(),
    'head': head,
    'inventory_captured_at_utc': data['captured_at_utc'],
    'status': 'passed' if not errors else 'failed',
    'scope': 'Static report integrity: source and document hashes, links/line ranges, counts, command registration, and decision crosswalk. This is not application compilation or runtime acceptance.',
    'metrics': metrics,
    'links_checked': links_checked,
    'decision_entries_checked': len(decision_ids),
    'known_source_wiring_gaps': unknown,
    'errors': errors,
}
(OUT / 'validation.json').write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(json.dumps(receipt, ensure_ascii=False, indent=2))
raise SystemExit(bool(errors))
