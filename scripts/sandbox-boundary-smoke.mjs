import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import readline from "node:readline";
import { randomUUID } from "node:crypto";

const helper = await readFile("scripts/sandbox-helper.py", "utf8");
const root = await mkdtemp(path.join(tmpdir(), "kanzei-sandbox-boundary-"));
const inputs = path.join(root, "inputs");
await mkdir(inputs);
await writeFile(path.join(inputs, "input.txt"), "EXPLICIT_INPUT_BYTES");
await writeFile(path.join(root, "outside.txt"), "HOST_SECRET_CANARY");
const linux = value => process.platform === "win32" ? execFileSync("wsl.exe", ["--exec", "/usr/bin/wslpath", "-a", value], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim() : value;
const command = script => process.platform === "win32" ? ["wsl.exe", ["--exec", "/usr/bin/python3", "-I", "-u", "-c", script]] : ["/usr/bin/python3", ["-I", "-u", "-c", script]];
const checks = [];
const check = (value, label) => { assert(value, label); checks.push(label); };

async function execute(script, control) {
  const [exe, args] = command(helper);
  const child = spawn(exe, args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const events = [], errors = [];
  child.stderr.on("data", bytes => errors.push(bytes));
  child.stdin.write(`${JSON.stringify({ command: script, inputs: linux(inputs), timeout_ms: control === "timeout" ? 200 : 10000 })}\n`);
  const lines = readline.createInterface({ input: child.stdout });
  for await (const line of lines) {
    const event = JSON.parse(line);
    events.push(event);
    if (event.event === "started" && control === "cancel") child.stdin.write("cancel\n");
    if (event.event === "started" && control === "eof") child.stdin.end();
  }
  const code = child.exitCode ?? await new Promise(resolve => child.once("exit", resolve));
  child.stdin.end();
  assert.equal(code, 0, Buffer.concat(errors).toString("utf8"));
  const result = events.find(item => item.event === "result")?.result;
  assert(result, "helper returned its durable execution receipt");
  check(result.isolation_verified === true, `actual namespace isolation: ${control || "boundary"}`);
  return result;
}

const escapedOutside = JSON.stringify(linux(path.join(root, "outside.txt")));
const inspection = `
import os, pathlib, socket
assert pathlib.Path('input.txt').read_text() == 'EXPLICIT_INPUT_BYTES'
assert not pathlib.Path(${escapedOutside}).exists()
assert not pathlib.Path('/mnt/c').exists()
assert not pathlib.Path('/run/WSL').exists()
assert not pathlib.Path('/init').exists()
assert not os.environ.get('WSL_INTEROP')
for fd in range(3, 256):
    try:
        os.fstat(fd)
    except OSError:
        continue
    raise AssertionError('inherited outside file descriptor: ' + str(fd))
try:
    pathlib.Path('/input/input.txt').write_text('forged')
    raise AssertionError('immutable input was writable')
except OSError:
    pass
assert list(pathlib.Path('/sys/class/net').glob('*')) == []
assert '00000000' not in pathlib.Path('/proc/net/route').read_text().splitlines()[1:]
with socket.socket() as client:
    client.settimeout(0.2)
    assert client.connect_ex(('1.1.1.1', 443)) != 0
pathlib.Path('created.txt').write_text('DISPOSABLE_OUTPUT')
print('BOUNDARY_VERIFIED')
`;
const quoted = value => `'${value.replaceAll("'", "'\\''")}'`;
let result = await execute(`/usr/bin/python3 -c ${quoted(inspection)}`);
check(result.exit_code === 0 && result.output.includes("BOUNDARY_VERIFIED"), "explicit input, immutable mount, absent host/interop, denied socket and disposable write");
check((await readFile(path.join(inputs, "input.txt"), "utf8")) === "EXPLICIT_INPUT_BYTES", "host input bytes unchanged");
check((await readFile(path.join(root, "outside.txt"), "utf8")) === "HOST_SECRET_CANARY", "outside canary unchanged");
check(result.published_files.length === 0 && result.host_writes === "none", "no sandbox files published to the host");
result = await execute("dd if=/dev/zero of=too-large bs=1048576 count=65 2>&1");
check(result.exit_code !== 0 && result.output.includes("No space left"), "workspace writes limited by a real 64 MiB tmpfs");

for (const control of ["complete", "timeout", "cancel", "eof"]) {
  const marker = `kanzei-sandbox-orphan-${randomUUID()}`;
  result = await execute(`setsid /bin/sh -c 'while :; do sleep 1; done' ${marker} & ${control === "complete" ? "exit 0" : "wait"}`, control);
  check(result.status === (control === "complete" ? "completed" : control === "timeout" ? "timeout" : "cancelled"), `cleanup acknowledged before ${control} result`);
  const probe = `import os,pathlib; marker=${JSON.stringify(marker)}.encode(); found=[p.name for p in pathlib.Path('/proc').iterdir() if p.name.isdigit() and int(p.name)!=os.getpid() and (p/'cmdline').exists() and marker in (p/'cmdline').read_bytes()]; print(found); assert not found`;
  const [exe, args] = command(probe);
  execFileSync(exe, args, { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  checks.push(`detached descendants gone after ${control}`);
}

const output = path.resolve("output/remediation-20261018/sandbox-boundary.json");
await writeFile(output, JSON.stringify({ status: "passed", root, checks }, null, 2));
console.log(`Sandbox OS boundary PASS: ${checks.length} checks`);
