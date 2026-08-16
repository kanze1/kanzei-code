#!/usr/bin/env node
// D-408:含中文的 .ps1 必须以 UTF-8 BOM 保存。
//
// 为什么是硬校验:Windows PowerShell 5.1(`powershell.exe`,系统自带、双击/`-File`
// 走的就是它)读无 BOM 的文件时按系统 ANSI 代码页(中文机器= GBK)解码,UTF-8 的
// 中文字节被拆成乱码,字符串里的引号错位 → 整个脚本 ParserError 直接跑不起来。
// PowerShell 7(`pwsh`)默认 UTF-8 所以本机开发一路正常,**只有用户按文档复制那条
// 命令时才炸** —— 2026-08-16 install-setup.ps1 实况:用户装紧急修复版时脚本解析
// 失败,而同批的 verify/release 在 agent 侧(PS7)全绿,典型的"开发机测不出"。
//
// 判据:文件含 CJK 字符(U+4E00–U+9FFF)即要求头三字节为 EF BB BF。纯 ASCII 脚本
// 不受约束(无 BOM 也不会被误解码)。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");
const scriptsDir = path.join(root, "scripts");

const offenders = [];
let checked = 0;
for (const name of fs.readdirSync(scriptsDir)) {
  if (!name.endsWith(".ps1")) continue;
  const file = path.join(scriptsDir, name);
  const buf = fs.readFileSync(file);
  const hasBom = buf.length >= 3 && buf[0] === 0xef && buf[1] === 0xbb && buf[2] === 0xbf;
  const text = buf.toString("utf8");
  const cjk = (text.match(/[一-鿿]/g) || []).length;
  checked += 1;
  if (cjk > 0 && !hasBom) {
    offenders.push(`scripts/${name}(含 ${cjk} 个中文字符,缺 UTF-8 BOM)`);
  }
}

if (offenders.length > 0) {
  console.error("PowerShell 脚本 BOM 校验失败——下列脚本在 Windows PowerShell 5.1 下会因编码解析报错:");
  for (const o of offenders) console.error(`  - ${o}`);
  console.error(
    "修复:用 UTF-8 with BOM 重存(PowerShell:\n" +
      "  $p='scripts\\<name>.ps1'; $t=[IO.File]::ReadAllText($p,[Text.Encoding]::UTF8);\n" +
      "  [IO.File]::WriteAllText($p, $t, (New-Object Text.UTF8Encoding $true))\n" +
      ")。"
  );
  process.exit(1);
}

console.log(`D-408 通过:${checked} 个 .ps1 脚本,含中文者均带 UTF-8 BOM`);
