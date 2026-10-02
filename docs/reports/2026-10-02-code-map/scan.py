"""Rebuild the static source inventory. No execution of application code."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import tomllib
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
EXTENSIONS = {'.rs', '.js', '.mjs', '.cjs', '.css', '.html', '.py', '.ps1', '.toml', '.json', '.yml', '.yaml', '.sh', '.cmd', '.bat', '.md', '.txt', '.mmd', '.xml', '.svg'}

def git(*args):
    return subprocess.check_output(['git', '-c', 'core.quotePath=false', '-C', str(ROOT), *args], stderr=subprocess.DEVNULL).decode('utf-8', errors='replace').strip()

def domain(path):
    if path.startswith('crates/'):
        return path.split('/')[1]
    if path.startswith('docs/'):
        return 'docs-auxiliary'
    return path.split('/')[0] if '/' in path else 'root'

def link(path, line=1):
    return f'[{path}](<{(ROOT / path).as_posix()}:{line}>)'

def cell(value):
    return value.replace('\\', '\\\\').replace('|', '\\|').replace('[', '\\[').replace(']', '\\]').replace('`', '\\`').replace('\n', ' ')

def collect():
    paths = []
    for name in ['crates', 'scripts', 'extras', '.github']:
        root = ROOT / name
        if root.exists():
            for p in root.rglob('*'):
                if p.is_file() and p.suffix.lower() in EXTENSIONS and not any(x in p.parts for x in ['vendor', 'node_modules', 'target', '__pycache__']):
                    paths.append(p)
    paths += [p for p in ROOT.iterdir() if p.is_file() and p.suffix.lower() in EXTENSIONS and p.name != 'package-lock.json']
    paths += [p for p in (ROOT / 'docs').rglob('*') if p.is_file() and p.suffix.lower() in {'.rs', '.js', '.mjs', '.cjs', '.css', '.html', '.py', '.ps1', '.sh'} and OUT not in p.parents]
    paths += [ROOT / '.cargo/config.toml']
    tracked = set(git('ls-files').splitlines())
    changed = set(git('diff', '--name-only', 'HEAD').splitlines())
    files = []
    for p in sorted(set(paths)):
        rel = p.relative_to(ROOT).as_posix()
        raw = p.read_bytes()
        text = raw.decode('utf-8-sig', errors='replace')
        lines = text.splitlines()
        symbols = []
        for n, line in enumerate(lines, 1):
            if p.suffix == '.rs':
                match = re.search(r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:fn|struct|enum|trait|type|const|static|mod)\s+([\w\u0080-\uffff]+)', line)
            else:
                match = re.search(r'^\s*(?:export\s+)?(?:async\s+)?(?:function|class|def)\s+([\w\u0080-\uffff]+)', line)
            if match:
                symbols.append({'name': match.group(1), 'line': n, 'signature': line.strip()})
        headers = []
        for line in lines[:45]:
            t = line.strip()
            if t.startswith(('//!', '//', '# ')):
                headers.append(t.lstrip('/#! ').strip())
            elif headers and t and not t.startswith(('#[', '/*', '*')):
                break
        tests = []
        for m in re.finditer(r'#\[(?:(?:tokio|async_std)::)?test[^\]]*\][\s\S]{0,500}?\bfn\s+([\w\u0080-\uffff]+)', text):
            tests.append({'name': m.group(1), 'line': text.count('\n', 0, m.start()) + 1})
        files.append({'path': rel, 'domain': domain(rel), 'lines': len(lines), 'sha256': hashlib.sha256(raw).hexdigest(), 'status': 'untracked' if rel not in tracked else 'modified' if rel in changed else 'HEAD', 'header': headers, 'symbols': symbols, 'tests': tests})
    main = (ROOT / 'crates/kanzei-app/src/main.rs').read_text(encoding='utf-8')
    block = re.search(r'tauri::generate_handler!\[([\s\S]*?)\]\)', main)
    registered = [x.strip() for x in block.group(1).split(',') if re.fullmatch(r'[A-Za-z_]\w*(?:::\w+)*', x.strip())] if block else []
    commands = []
    for f in files:
        if f['path'].startswith('crates/kanzei-app/src/'):
            txt = (ROOT / f['path']).read_text(encoding='utf-8-sig')
            for m in re.finditer(r'^\s*#\[tauri::command[^\]]*\][\s\S]{0,500}?\bfn\s+(\w+)', txt, re.M):
                name = m.group(1)
                rename = re.search(r'rename\s*=\s*"([^"]+)"', m.group(0))
                commands.append({'name': name, 'command_name': rename.group(1) if rename else name, 'path': f['path'], 'line': txt.count('\n', 0, m.start()) + 1, 'registered': any(x.split('::')[-1] == name for x in registered)})
    invokes = []
    for f in files:
        if f['path'].startswith('crates/kanzei-app/ui/') and f['path'].endswith(('.js', '.html')):
            txt = (ROOT / f['path']).read_text(encoding='utf-8-sig')
            for m in re.finditer(r'\binvoke\(\s*[\'\"]([^\'\"]+)[\'\"]', txt):
                invokes.append({'name': m.group(1), 'path': f['path'], 'line': txt.count('\n', 0, m.start()) + 1})
    tables = []
    for f in files:
        if f['path'].endswith('.rs'):
            txt = (ROOT / f['path']).read_text(encoding='utf-8-sig')
            for m in re.finditer(r'CREATE TABLE(?: IF NOT EXISTS)?\s+(\w+)', txt, re.I):
                tables.append({'name': m.group(1), 'path': f['path'], 'line': txt.count('\n', 0, m.start()) + 1})
    totals = {}
    for f in files:
        row = totals.setdefault(f['domain'], {'files': 0, 'lines': 0, 'test_declarations': 0})
        row['files'] += 1
        row['lines'] += f['lines']
        row['test_declarations'] += len(f['tests'])
    dependencies = {}
    for manifest in (ROOT / 'crates').glob('*/*'):
        if manifest.name.lower() != 'cargo.toml':
            continue
        crate = tomllib.loads(manifest.read_text(encoding='utf-8-sig'))
        dependencies[crate['package']['name']] = {key: value for key, value in crate.get('dependencies', {}).items() if key.startswith('kanzei')}
    resources = []
    for name in ['Cargo.lock', 'package-lock.json', '.gitignore']:
        p = ROOT / name
        if p.is_file():
            resources.append({'path': name, 'bytes': p.stat().st_size})
    for name in ['crates', 'scripts', 'extras']:
        for p in (ROOT / name).rglob('*'):
            if p.is_file() and p.suffix.lower() not in EXTENSIONS and not any(x in p.parts for x in ['vendor', 'node_modules', 'target', '__pycache__']):
                resources.append({'path': p.relative_to(ROOT).as_posix(), 'bytes': p.stat().st_size})
    documents = []
    for p in sorted((ROOT / 'docs').rglob('*.md')):
        if OUT in p.parents:
            continue
        txt = p.read_text(encoding='utf-8-sig', errors='replace')
        title = re.search(r'^#\s+(.+)', txt, re.M)
        state = re.search(r'^.*(?:状态|Status)[:：].*$', txt, re.M)
        documents.append({'path': p.relative_to(ROOT).as_posix(), 'title': title.group(1) if title else p.stem, 'stated_status': state.group(0).strip() if state else '', 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()})
    data = {'captured_at_utc': datetime.now(timezone.utc).isoformat(), 'head': git('rev-parse', 'HEAD'), 'branch': git('branch', '--show-current'), 'scope': 'Handwritten crate sources, prompts, manifests, root configuration, .cargo build config, scripts, .github, and auxiliary code/prototypes under docs. Third-party vendor files, lockfile contents, and project runtime/user data excluded. Binary resources and lockfiles listed separately; docs metadata catalogued separately.', 'method': 'Static regex inventory; symbols and links are navigation aids, not compiler reachability or runtime acceptance.', 'totals': totals, 'files': files, 'resource_files': resources, 'documents': documents, 'crate_dependencies': dependencies, 'tauri_registered': registered, 'tauri_commands': commands, 'ui_literal_invokes': invokes, 'sql_table_declarations': tables}
    (OUT / 'inventory.json').write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    parts = ['# 全量源码文件索引', '', '每个纳入扫描的文件都列在这里。行数包含注释与测试；符号索引见 inventory.json。第三方 vendor、二进制资源和运行数据不计入手写源码。', '']
    for d, stats in totals.items():
        parts += [f'## {d}', '', f"{stats['files']} 个文件，{stats['lines']} 行。", '', '| 文件 | 行数 | Git 状态 | 文件说明或主要符号 |', '|---|---:|---|---|']
        for f in files:
            if f['domain'] != d:
                continue
            summary = ' '.join(f['header'])[:240] or ', '.join(s['name'] for s in f['symbols'][:8])
            summary = cell(summary)
            parts.append(f"| {link(f['path'])} | {f['lines']} | {f['status']} | {summary} |")
        parts += ['']
    (OUT / '08-file-index.md').write_text('\n'.join(parts), encoding='utf-8')
    parts = ['# 桌面命令与数据表索引', '', '从当前源码机械提取。动态 invoke 与间接注册需结合调用链阅读；重复表声明可能来自迁移或测试。', '', '## Tauri 命令', '', '| 命令 | 声明 | main 注册 | 前端字面量调用数 |', '|---|---|---|---:|']
    for c in commands:
        count = sum(x['name'] == c['command_name'] for x in invokes)
        parts.append(f"| {c['command_name']} | {link(c['path'], c['line'])} | {'是' if c['registered'] else '未匹配'} | {count} |")
    names = {c['command_name'] for c in commands}
    unknown = [i for i in invokes if i['name'] not in names]
    parts += ['', '## 前端字面量调用未匹配后端声明', '', '| 调用 | 位置 |', '|---|---|']
    for i in unknown:
        parts.append(f"| {i['name']} | {link(i['path'], i['line'])} |")
    if not unknown:
        parts.append('| 无 | — |')
    parts += ['', '## SQL 表声明', '', '| 表名 | 位置 |', '|---|---|']
    for t in tables:
        parts.append(f"| {t['name']} | {link(t['path'], t['line'])} |")
    (OUT / '09-interface-index.md').write_text('\n'.join(parts) + '\n', encoding='utf-8')
    parts = ['# 全量符号和测试声明索引', '', '源码中的函数、类型、常量和模块声明逐项链接。机械提取包含内部函数和测试；宏生成、闭包和箭头函数不能视为已完整解析。实现逻辑的人工说明在 02 到 06，逐函数实现可由此直接打开源码。', '']
    for f in files:
        if not f['symbols']:
            continue
        parts += [f"## {f['path']}", '', '| 名称 | 声明 |', '|---|---|']
        for s in f['symbols']:
            sig = s['signature'].replace('|', '\\|').replace('`', '')
            parts.append(f"| {link(f['path'], s['line']).replace(f['path'], s['name'], 1)} | `{sig}` |")
        parts += ['']
    (OUT / '10-symbol-index.md').write_text('\n'.join(parts), encoding='utf-8')
    parts = ['# 文档与资源索引', '', '文档状态字段按原文列出，未以文字状态证明实现或验收。资源只列路径和大小，不读取本地 profile 或二进制内容。第三方 vendor 和缓存不在此表。', '', '## 现有文档', '', '| 文档 | 原标题 | 文中状态 |', '|---|---|---|']
    for d in documents:
        title = cell(d['title'])
        state = cell(d['stated_status'])[:300]
        parts.append(f"| {link(d['path'])} | {title} | {state} |")
    parts += ['', '## 资源和其他文件', '', '| 文件 | 字节数 |', '|---|---:|']
    for r in sorted(resources, key=lambda x: x['path']):
        parts.append(f"| {link(r['path'])} | {r['bytes']} |")
    (OUT / '11-documents-assets.md').write_text('\n'.join(parts) + '\n', encoding='utf-8')
    print(json.dumps({'head': data['head'], 'branch': data['branch'], 'totals': totals, 'files': len(files), 'tauri_registered': len(registered), 'command_declarations': len(commands), 'unmatched_commands': [c['name'] for c in commands if not c['registered']], 'sql_tables': len(set(t['name'] for t in tables))}, ensure_ascii=False, indent=2))

if __name__ == '__main__':
    collect()
