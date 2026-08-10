# 第一梯队实施方案(D-267 / R-183 / R-177 / R-182 内容②)

- 日期: 2026-08-11
- 状态: **待用户拍板三项**(见文末「需拍板」)
- 来源: 12 agent 工作流(6 勘察 + 2 独立方案 + 3 对抗证伪 + 1 收敛),2.35M token。勘察产出事实 202 条、红线 57 条;证伪挖出致命 9 条,7 条改方案、2 条接受并收窄。
- 关联: docs/design/parallel_lines_ui.md(前端与依赖链) docs/design/parallel_read_serial_write_orchestration.md(口径基线)

## 总体

以方案B(语义正确优先)为骨架,嫁接方案A的低风险取舍(project_dir 恒主根、不改「总是允许」落盘、不做 session_id 后缀),并逐条消化 9 条 fatal。实测复核确认的三件关键事实决定了最终形态:①permission.rs:209-213 的 gate 会先行 return false,两份方案的兼容垫片都是死代码——垫片必须**放在 gate 之前且只做逐字节相等**(不走 wildcard),这样既救活 12 条历史规则,又让 :494 的性质由这一行独立保住;②`generalize_resource` 已确认是恒等函数、`session_rules` 存整串 JSON,所以「总是允许」的落盘形态**本轮一个字不改**,否则本轮生效重启失效;③`resources_with_ctx` 用 `ctx.cwd.join(workdir)` 造资源,cwd 一旦指向 worktree,那 12 条钉死主根 workdir 的规则一条都不命中——这是 R-183 验收③,两份方案都零覆盖,本轮靠「form C 的 workdir 显式表达 + per-run allowlist 的 workdir 由 CLI 钉成本次 cwd + 启动告警点名失配」三件事合起来满足,并把验收③ 重述成可判定形态。最重的一条是证伪二:段级闸门是纯语法过滤器,`cargo *` 对一个能改源码的 agent 就是任意代码执行,任何黑名单都关不掉。本轮的处置是**不假装它是安全边界**:把闸门的契约明写成「只保证被调起的程序与工作目录是操作员写下的那一组」,新增 ACE 类程序的启动告警,并把 relocation/config 注入类 flag(-C/--git-dir/--work-tree/--manifest-path/--config/--target-dir 等,大小写不敏感)纳入否决表——这一层守的是「workdir 是授权身份的一部分」这条真红线,是可测的;ACE 残余风险如实标注、需用户拍板。共 15 批,分 7 波,其中 5 波可任务级并行。

## 批次(15)

### F0 · D-267 地基 · Rule 结构加宽:command/workdir 两个可选字段,零行为变化(全仓编译面,必须独占派发)

- 文件面: `crates/kanzei-harness/src/permission.rs`, `crates/kanzei-harness/src/harness.rs`, `crates/kanzei-harness/src/config.rs`, `crates/kanzei-tools/src/write.rs`
- 改动: 1) `Rule` 改为 `{ action: String, #[serde(default)] resource: String, pub effect: Effect, #[serde(default, skip_serializing_if="Option::is_none")] command: Option<String>, #[serde(default, skip_serializing_if="Option::is_none")] workdir: Option<String> }`。`skip_serializing_if` 是硬要求:settings.rs 的 `permission_rule_delete` 走整文件 serde round-trip,缺了它会给用户现有 24 条规则各物化两个空字段(实测该文件 24 条,其中 21 条 action=bash)。
2) 加两个构造器:`Rule::exact(action, resource, effect)`(旧两档)与 `Rule::command(action, command, workdir, effect)`(第三档)。**不给 Rule 实现 Default**——`effect` 没有安全的默认值,`..Default::default()` 会让新代码沉默地拿到某个 effect。全仓 27 处 `Rule { .. }` 字面量(实测:permission.rs 23、harness.rs 2、config.rs 1、write.rs 1)全部改走构造器。
3) `harness::rule(action, resource, effect)` 助手体内改调 `Rule::exact`,**签名不变**——它有 34 个调用点(base.rs 12、profiles.rs 15、subagent.rs 4、harness_ext.rs 2、memory/manager.rs 1),签名一动就是 5 个文件的连带改动,本批坚决不动。
4) config.rs `unknown_keys` 里 `permissions.rules[i]` 的已知键清单加 `"command"`、`"workdir"`。
5) **本批不新增任何判定逻辑**:`command.is_some()` 的规则此刻还没有任何代码读它,行为与改前逐字节相同。
本批必须独占一波:给 Rule 加字段会打断 workspace 全部下游的结构体字面量,任何与它同批派发的线都编不过。
- 测试:
  - permission.rs 新增 `新字段不改变任何既有判定`:对 :442/:465/:476/:494/:517/:549/:590/:601 八条既有用例各跑一遍构造器改写后的版本,断言 Effect 与改前逐字相同(改写只换构造方式,断言一字不动)
  - config.rs 新增 `新字段不被序列化物化`:把当前 .kanzei/kanzei.toml 的 24 条规则做成 fixture,serde round-trip 后 `assert!(!toml_text.contains("command"))` 且 `assert!(!toml_text.contains("workdir ="))`(守 D-083 的保存不丢字段/不长字段)
  - config.rs 新增 `新形态字段不被报成未知键`:一条写了 command/workdir 的规则不出现在 unknown_keys 输出里
  - 既有 `unknown_keys_schema_matches_struct` 与 `unknown_fields_are_tolerated_and_reported` 保持绿
- 验收: ① `cargo build --workspace` 通过(结构体字面量漏字段编译不过,这是本批唯一需要的机械保证);② `grep -rn 'Rule {' crates --include=*.rs` 零命中(全部走构造器);③ `grep -n 'skip_serializing_if' crates/kanzei-harness/src/permission.rs` 恰 2 处;④ `grep -n 'impl Default for Rule' crates/kanzei-harness/src/permission.rs` 零命中;⑤ `cargo test --workspace` 全绿、`cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 无输出。

### F1 · D-267(证伪一 FATAL-1 的处置) · bash 资源不再被当路径规范化 + 兼容垫片放到 gate 之前且只做逐字节相等

- 文件面: `crates/kanzei-harness/src/permission.rs`, `crates/kanzei-core/src/runner/drive.rs`
- 改动: 1) permission.rs 新增 `pub fn normalize_resource_for_action(action: &str, resource: &str) -> String`:`action == "bash"` 时**原样返回**,其余委托 `normalize_resource`。函数注释写清事实:bash 的 resource 由 `BashTool::resources_with_ctx`(bash.rs:88-99)造成 `{"command":…,"workdir":…}`,其中 workdir **已经**单独过了一次 `normalize_resource`;对整条 JSON 串再跑一次路径规范化只有损失——Windows 下整串小写、`\"` 折成 `/"` 使 JSON 失效、`//` 折叠、`a/../b` 被消解。
2) drive.rs 三处调用点全换成新函数,**必须同批**:并行预检 :545、并行 deny 扫描 :604、串行门禁 :764。漏一处两个评估站点就语义分裂。路径类 action(write/edit/read/glob/grep)逐字节不变。
3) **`resource_match_for_action` 的 bash 分支改成下面这个顺序**(这是本批的核心,也是两份方案共同的致命错处):
```rust
if action == "bash" {
    if pattern == "*" { return wildcard_match(pattern, value); }   // :200 直通,不动
    let vs = /* value 解析成含 command+workdir 的 JSON */;
    let ps = /* pattern 同上 */;
    // ① D-267 兼容垫片,必须在 gate 之前:2026-08-11 前落盘的 pattern 是
    //    drive.rs 规范化后的 mangled 形态。只允许与 normalize_resource(value)
    //    **逐字节相等**,不走 wildcard —— 因此 "git status" / "git *" 这类真正的
    //    legacy 纯字符串 pattern 永远无法凭此命中结构化请求,:494 的性质由这一行
    //    独立保住,而不是靠下面的 gate。
    if vs && pattern == normalize_resource(value) { return true; }
    // ② 原 gate,一字不改地保留在垫片之后
    if vs && !ps { return false; }
    return wildcard_match(pattern, value);
}
```
**为什么两份原方案都错**:它们把垫片放在 `wildcard_match` 那一行(gate 之后)。实测复核:配置第 89 行等 7 条 pattern 含 `/"`,JSON 解析**失败**;停止 mangling 后 value 解析**成功** → gate 先行 `return false`,垫片永远执行不到,两份方案自列的向后兼容测试当场红,并会逼实施者去改 :209-213 —— 那正是 :494 唯一的执行机制。
**为什么放前面是安全的**:垫片的准入集是 `{V : P 逐字节等于 normalize_resource(V)}`,而 normalize_resource 是确定性函数,每个 V 只对应唯一一个 P。没有通配、没有等价类扩张。与方案A「两侧都 normalize」不同——那是把准入集从 `{V: N(V)==P}` 放宽成 `{V: N(V)==N(P)}`,对未规范化的 pattern 是真实放宽(如配置第 39 行 `./scripts/release.ps1` 会从「永不命中」变成「命中」),方向错误,本轮不采。
4) 垫片同时覆盖两类历史规则,实测已核对:5 条 pattern 是合法 JSON 但被整串小写过(如第 82 行的 `select-string ... select-object`),停止 mangling 后 value 带原始大小写、`wildcard_match` 逐字节比会失配,靠垫片救回;7 条 pattern 因 `/"` 不是合法 JSON,靠垫片跨过 gate。
- 测试:
  - permission.rs 新增 `gate守卫_结构化value不被非结构化pattern授权`:**直接测 `resource_match_for_action` 本身**(不经 evaluate),pattern 取 `git status`/`git *`/`*.md`/`cargo *` 四个非结构化形态,value 取合法结构化 JSON,断言全部 false。这条是本轮唯一钉住 :209-213 那道 gate 的单元守卫——今天该性质只由 :494 一条端到端测试间接覆盖,而本轮正在它紧邻处动刀(证伪一 missing 第 1 条的直接落点)
  - permission.rs 新增 `历史mangled规则向后兼容_12条逐条`:把当前 .kanzei/kanzei.toml 里 12 条 bash 结构化规则的 resource 原文做成 fixture(含第 79/82/89/94/104 等行),对每条构造其**未 mangled** 的真实 value,逐条断言 Allow。12 个 case,一条不许合并(D-267 验收④的机械形态)
  - permission.rs 新增 `垫片只做逐字节相等_不做通配扩张`:pattern 取 `{"command":"cargo *","workdir":"*"}` 的 **mangled** 形态,value 取一条不同的 cargo 命令 → 断言 false(证明垫片不是通配通道)
  - permission.rs 新增 `bash资源进evaluate时与BashTool产出逐字节相同`:含 `\"` 与大写盘符的命令,断言 `normalize_resource_for_action("bash", r)` == r
  - permission.rs 既有 :442/:465/:472/:476/:494 **一行不改**保持绿;D-050 六条路径测试(:303/:325/:354/:382/:412/:423)与 write.rs 落点一致性测试一字不改保持绿
  - drive.rs 新增 `三个评估站点对同一bash请求给出同一Effect`:同一 resource 分别过并行预检、并行 deny 扫描、串行门禁的规范化路径,断言三者得到的字符串相同
- 验收: ① `grep -n 'permission::normalize_resource(&resource)' crates/kanzei-core/src/runner/drive.rs` 零命中,`grep -c 'normalize_resource_for_action' crates/kanzei-core/src/runner/drive.rs` == 3;② `git diff crates/kanzei-harness/src/permission.rs` 在 :198-216 区间内新增行全部位于 `if vs && !ps { return false; }` **之前**,该行本身零改动(人工可核,但由 `gate守卫` 测试机械兜底);③ `cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools -p kanzei` 全绿,`向后兼容_12条` 的 12 个 case 逐条出现在 `cargo test -p kanzei-harness -- --nocapture` 输出;④ fmt/clippy 全绿(`-p kanzei-harness -p kanzei-core --all-targets -- -D warnings`)。

### F2 · D-267 地基(纯新增,零接线) · 保守 shell 词法器 cmdline.rs:切段 + 否决表(含 relocation flag,证伪一 FATAL-3 的处置)

- 文件面: `crates/kanzei-harness/src/cmdline.rs`, `crates/kanzei-harness/src/lib.rs`
- 改动: 新建零依赖模块,不被任何人调用,独立可提交。
```rust
pub enum Scan { Segments(Vec<String>), Unsupported(&'static str) }
pub fn scan(command: &str) -> Scan;
pub fn tokens(segment: &str) -> Option<Vec<String>>;   // 引号感知,未闭合引号 → None
pub const ESCAPE_TOKENS: &[&str];        // 快照测试钉死
pub const RELOCATION_FLAGS: &[&str];     // 同上
pub const ACE_PROGRAMS: &[&str];         // 供 F8 的告警复用
```
三态状态机(无引号 / 单引号 / 双引号):单引号内全字面;双引号内 `$(`/`${`/`@(`/反引号 → `Unsupported("command-substitution")`,**裸 `$VAR` / `$?` 不算**(实测清单① `echo "EXIT=$?"` 要能过);无引号态:`;` `&&` `||` `|` `\n` `\r` 是段分隔符,单个 `&` → `Unsupported("background")`,`>` `>>` `<` `2>` `|&` → `Unsupported("redirection")`,`$(` `${` `@(` 反引号 → `Unsupported("command-substitution")`,`(` `)` `{` `}` → `Unsupported("script-block")`,`#` → `Unsupported("comment")`,未闭合引号 → `Unsupported("unterminated-quote")`。段两端 trim,空段丢弃,切完无非空段 → Unsupported。
**逃逸/重定位表(全部大小写不敏感,同时匹配 `--flag` 与 `--flag=value` 两种形态)**——这是证伪一 FATAL-3 与证伪二 FATAL-2 的直接落点:
- 通用逃逸(任意位置):`-c` `--command` `-Command` `-EncodedCommand` `--eval` `-exec` `--exec` `eval` `iex` `Invoke-Expression`;任一 token **内含 `!`**(接住 `git -c alias.x=!calc x`);段的**首 token** 是 `.` 或 `source`。
- **重定位/配置注入**(任意位置):`-C` `--directory` `--chdir` `--cd` `--workdir` `--work-tree` `--git-dir` `--manifest-path` `--config` `--target-dir` `--project` `--prefix` `--root`。理由:这一类 flag 让命令在**规则里写的那个 workdir 之外**取工作目录或配置,直接架空「workdir 是授权身份的一部分」这条红线,而两份原方案的表里一条都没有。证伪二给出的 `cargo build --config <攻击者写的 toml>`(rustc-wrapper 注入)与 `cargo run --manifest-path <外部 crate>`(build.rs 构建期执行)由 `--config`/`--manifest-path` 命中。
- `-e` **只在**段首 token ∈ {node, python, python3, perl, ruby, php, deno, bun} 时才算逃逸。**不得无条件否决 `-e`,也不得否决非首位的裸 `.`** —— 否则 `grep -e pat`、`git add .` 全部被拦,F14 的端到端闭环走不完(证伪一 concerns 第 6 条:被迫回来放宽 veto 表就是放宽安全边界,必须一次写对)。
模块头注释必须写死本模块的**契约边界**:「本模块只回答『这条命令由哪几个程序、在哪个目录被调起』,**不回答『被调起的程序会不会执行任意代码』**。放行一个构建器/解释器(cargo/node/pwsh/awk/python)等于在该 workdir 授予任意代码执行,这是程序语义,任何 shell 语法过滤器都关不掉——见 F8 的 ACE 告警。」
- 测试:
  - 表驱动 `切分口径`:`a;b` `a&&b` `a||b` `a|b` 多行各切两段;`a;;b` 与末尾 `;` 的空段被丢弃;`echo "a;b"` 与 `echo 'a&&b'` 各恰 1 段
  - 表驱动 `不可静态判定构造` ≥12 条:`>` `>>` `<` `2>&1` `$(...)` `${x}` 反引号 `&` `(` `{` `#` 未闭合引号,各断言 Unsupported 且理由字符串正确
  - `裸美元变量与问号不算命令替换`:`echo "EXIT=$?"` → Segments 且 1 段(实测清单①)
  - 表驱动 `实测被拒清单五条`:①`node s.js && echo "EXIT=$?"`→2 段;②`ls p | head -0; ls p`→3 段;③`awk 'prog' f`→1 段;④`cargo test | Select-Object -Last 40`→2 段;⑤`... | head -30; echo x`→3 段(D-267 验收⑦的输入被固化成用例)
  - 表驱动 `逃逸token`:`python -c x`、`pwsh -Command x`、`git -c alias.x=!calc x`、`node --eval x`、`. ./evil.ps1`、`source x` 全部命中且理由点名 token;**反向**:`grep -e pat`、`git add .`、`sed -e s/a/b/` 全部**不**命中
  - 表驱动 `重定位flag`(证伪一 FATAL-3 / 证伪二 FATAL-2 的定向反证):`git -C ../other reset --hard`、`git --git-dir=../o/.git --work-tree=../o clean -fdx`、`cargo --manifest-path ../o/Cargo.toml test`、`cargo build --config C:/x.toml`、`cargo build --target-dir ../o`、`pwsh -WorkingDirectory ..` 全部命中重定位表;大小写变体 `-c`/`-C`/`--CONFIG` 三种写法各一条
  - `表内容快照`:`assert_eq!(ESCAPE_TOKENS, &[...])` 与 `assert_eq!(RELOCATION_FLAGS, &[...])` 两条字面量快照断言,并在注释里写死纪律:**给表减项必须同批新增一条该项对应的反例测试**。这是证伪一 missing 第 4 条(安全边界不能只靠模块注释里的一句话维持)的机械落点
  - `tokens 引号成词`:`awk 'a b' f` → ["awk","a b","f"];未闭合引号 → None
- 验收: ① `cargo tree -p kanzei-harness --depth 1` 与改前逐行相同(零新依赖);② `cargo test -p kanzei-harness cmdline` 用例数 ≥ 40;③ `git diff --stat` 只显示 cmdline.rs(新增)与 lib.rs(一行 `pub mod cmdline;`),不改任何既有函数体;④ 表快照测试存在:`grep -c 'assert_eq!(ESCAPE_TOKENS' crates/kanzei-harness/src/cmdline.rs` ≥1 且 `grep -c 'assert_eq!(RELOCATION_FLAGS' ...` ≥1;⑤ fmt/clippy(`-p kanzei-harness --all-targets -- -D warnings`)全绿。

### F3 · R-182 内容② / 验收①②⑤ · kz 显式主根入口:--project-root 与 KANZEI_PROJECT_ROOT,收口 run/replay-eval/tracker 三条入口

- 文件面: `crates/kanzei-harness/src/config.rs`, `crates/kanzei/src/main.rs`, `crates/kanzei/tests/worktree_main_root.rs`
- 改动: 1) config.rs 新增 `pub fn load_with_warnings_at_root(project_root: &Path) -> anyhow::Result<(KanzeiConfig, Vec<String>)>` 与 `load_at_root`:直接 merge 全局 `kanzei_home()/kanzei.toml` 与 `project_root/.kanzei/kanzei.toml`,**不做任何根发现**。把现有 `load_with_warnings(cwd)` 改写成 `discover_project_root(cwd).unwrap_or(cwd)` 再委托 at_root 的薄包装 —— 17 个既有调用点一行不改、语义逐字节不变。
2) config.rs 新增 `pub fn resolve_project_root(explicit: Option<&Path>, cwd: &Path) -> anyhow::Result<PathBuf>`:explicit 有值时校验「存在 + 是目录 + 含 `.kanzei` 目录或 `.git`(worktree 的 `.git` 是文件,目录/文件都算)」,不满足则 `bail!` 并点名来源;**不做 canonicalize**(与 run.rs 现有理由同源:`\\?\` 形态会让用户已写的绝对路径规则一夜失配)。explicit 为 None 时 `discover_project_root(cwd).unwrap_or(cwd)`,即今天的行为。函数注释写死:`KANZEI_PROJECT_ROOT` 改的是**项目根**,`KANZEI_HOME` 改的是**全局根**,两者正交(D-187 教训)。
3) main.rs `parse_run_args` 返回类型从 `(bool, bool, String)` 改成 `RunArgs { new_session, readonly, project_root: Option<PathBuf>, prompt }`,三条既有断言测试**改写而非删除**。新增 `--project-root <path>`,解析时**把 flag 与它的值两个 token 都从 prompt 里剥掉**(新开关最常漏的一步)。
4) main.rs 新增 `fn explicit_main_root(flag: Option<&Path>) -> Option<PathBuf>`:flag 优先,否则读 `KANZEI_PROJECT_ROOT`(trim 后非空才算设置),与既有 KANZEI_PROFILE/AGENT/MODEL/PROXY 同构。
5) `run_cli`:取根提到配置加载**之前** —— `let project_root = resolve_project_root(explicit.as_deref(), &cwd)?; reject_home_as_project_root(&project_root)?; let (config, warnings) = load_with_warnings_at_root(&project_root)?;`(现状是 :123 先 load(cwd)、:137 才 discover,顺序对调)。`ToolCtx::new(cwd, project_root)` —— 两者第一次可能不等。
6) `replay_eval_cli` 同构改造。
7) `tracker_cli`:`ToolCtx::discovering(current_dir()?)` → `ToolCtx::new(cwd, resolve_project_root(explicit_main_root(None).as_deref(), &cwd)?)` + `reject_home_as_project_root`。**这一条是实测①「两棵树相隔 10 秒都拿到 D-267」的直接落点,漏掉它端到端不成立。**
8) `usage_text()` 补两行说明 flag 与 env。
9) **本批不动 `discover_project_root` 一个字符** —— tool.rs:248「worktree 内必须仍返回 worktree」那条有意固化危害前提的断言原样绿;净效果是 CLI 侧少了一个发现式取根调用点、没有新增任何一个。
- 测试:
  - config.rs `load_at_root不做根发现`:root 与其子目录 sub 各有一份 kanzei.toml(primary 不同),断言 `load_at_root(root)` 取 root 那份、`load_with_warnings(sub)` 仍取 sub 那份(两个入口语义不同且旧入口不变)
  - config.rs `resolve_project_root显式优先`:explicit=Some(root)、cwd=sub → 返回 root;explicit=None → 返回 discover 结果(同一条测试里同时断言两件事,证明没去改 discover_project_root)
  - config.rs `显式主根必须是真项目根`:不存在的路径 / 无 .kanzei 无 .git 的空目录 → 报错且错误文本点名来源键名
  - config.rs `显式主根不做canonicalize`:含尾分隔符与小写盘符的路径,返回值不带 `\\?\` 前缀
  - main.rs `project_root_flag_and_value_are_stripped_from_prompt`:`["--project-root","C:/x","hello","world"]` → prompt 恰为 `hello world`(防开关字面量当提示词发出去)
  - main.rs `explicit_main_root_prefers_flag_over_env`
  - main.rs `显式主根同样过HOME拦截`:指向 HOME → 报错(D-194 红线不被新入口绕过)
  - 集成测试 crates/kanzei/tests/worktree_main_root.rs `跨worktree登记落主根且副本零改动`:真 `git init` + `git worktree add`,在 worktree 内以 `KANZEI_PROJECT_ROOT=<主根>` 跑 `kz defect add`(免 LLM 路径,不需要 mock SSE),断言条目落主根 defects.md、worktree 内 `.kanzei/project/defects.md` 的 sha256 与运行前逐字节相同(R-182 验收①)
  - 同文件 `两个独立OS进程跨树并发登记编号互异`:两棵 worktree 各 spawn 一个真 OS 进程(**不用进程内多线程冒充** —— D-268 已把那条记成缺陷),for 循环跑 5 轮,每轮新建临时仓,断言 10 个编号互异、主根条目数 == 10(R-182 验收②;把「重跑实测①」这句人工动作换成机械判据)
  - 三条既有 parse_run_args 测试改写为新结构体形态(不删);usage 断言测试补 `--project-root` 子串
- 验收: ① `grep -n 'ToolCtx::discovering' crates/kanzei/src/main.rs` 零命中;② `grep -rn 'discover_project_root' crates/kanzei/src` 零命中;③ 集成测试里的 sha256 比对与 5 轮编号互异用例存在且绿;④ `cargo test -p kanzei-harness -p kanzei` 全绿;⑤ **新测试文件落在 crates/kanzei/tests/,clippy 必须带 `-p kanzei`**(D-264 的直接教训):`cargo fmt --all -- --check` + `cargo clippy -p kanzei -p kanzei-harness --all-targets -- -D warnings` 无输出。

### F4 · R-177 内容①⑥ / 验收①⑤ · process_create 建线:worktree_path 真实绑定、一树一线查重、失败整体回滚;定死 project_dir 恒主根

- 文件面: `crates/kanzei-app/src/processes.rs`, `crates/kanzei-app/src/worktree_tests.rs`, `crates/kanzei-core/src/store/schema.rs`
- 改动: 1) **先定死字段口径并写进注释**:`ProcessHandle.project_dir` 与 `origin_project` **恒为主根**,worktree 路径**只**由 `worktree_path` 承担。实测复核:processes.rs:158(`p{n}` 计数)、:226(persist 反推 root)、:240(session_id 反推 root)与 state.rs:336 四处都拿 project_dir 反推主根,改存 worktree 会让会话按树分裂、state.db 落进 worktree。据此**更正 store/schema.rs:184-186 的注释**(它今天写的是「project_dir 是执行工作树」,与代码事实相反)。这条选择比方案B「改 5 处判 origin_project」便宜且不碰 12 个 `process_session_id` 调用点。
2) 把 `worktree_create` 的 git 逻辑抽成非 Tauri 的 `pub(crate) fn create_worktree(root: &Path, name: &str) -> Result<WorktreeInfo, String>`(safe_name 过滤、目标存在即拒、`git worktree add -b kanzei/thread-<name> <path> HEAD`);Tauri 命令 `worktree_create` 保留、体内调它,行为不变。顺带补上今天缺的一件事:git 失败后**回收已建目录**。
3) `worktree_create` 返回的 `files`/`clean`/`diff` 今天是硬编码乐观值(空/true/空),改成真实探测(复用 `worktree_diff` 的实现)——否则收活流程会把「线还有活没提交」当干净合并。
4) `process_create` 增参数 `worktree_name: Option<String>`(Tauri 对缺省 Option 参数解析为 None,前端无需改动)。给定时:**先**在已有 handle 里按规范化路径查重(同一 worktree 已被绑定 → 直接返回错误并点名那条线的 id,**此时不建树**);再 `create_worktree`;再令 `worktree_path = Some(path)`;`persist_process` 失败则整体回滚 —— `git worktree remove --force` + `git branch -D` + 从内存表移除。
5) processes.rs 文件尾加 `#[cfg(test)] #[path = "worktree_tests.rs"] mod tests;`(R-177 验收⑦「processes.rs 不再零测试」的字面满足)。
- 测试:
  - worktree_tests.rs 夹具:temp_dir 下真 `git init` + `git commit --allow-empty`(沿用 process_tests.rs 手搓 temp 目录的风格,零新依赖;tool.rs:221 那套只伪造磁盘形态,不够)
  - `建线后worktree_path是真实路径`:Some 且目录存在、分支为 `kanzei/thread-<name>`(验收①前半)
  - `同一worktree不得绑定第二条线`:第二次报错且文案含已绑定线的 id;并断言**没有**新建目录(查重先于建树,验收⑤)
  - `落库失败时worktree被回收_不留半绑定态`:state.db 路径设为不可写触发 persist 失败,断言目录已删、分支已删、内存进程表无该 id(验收①后半)
  - `worktree_create返回的clean反映真实工作区`:留一个未提交改动 → clean == false
  - `不带worktree_name时行为与今天一致`:worktree_path 恒 None、`p{n}` 递增规则不变;既有 process_tests.rs 的两条 R-178 回归与 `勘察复核开关默认关闭` 保持绿
  - `线的session_id仍由主根算出`:worktree_path 有值时 session_id 与主树同进程一致(D-176 红线)
- 验收: ① `cargo test -p kanzei-app worktree -- --list` 的输出里 `worktree_create` 相关用例 ≥1(**不用 `grep -c 'fn worktree_'`** —— 证伪三已指出那条判据判不出验收⑦);② `grep -n 'mod tests' crates/kanzei-app/src/processes.rs` 有命中;③ schema.rs:184-186 的注释与代码事实一致(`grep -n 'project_dir 是执行工作树' crates/kanzei-core/src/store/schema.rs` 零命中);④ `cargo test -p kanzei-app -p kanzei-core` 全绿;⑤ fmt/clippy(`-p kanzei-app -p kanzei-core --all-targets -- -D warnings`)全绿。

### F5 · D-267 主体 / 验收①②③④ · argv 段级闸门:第三档规则的判定顺序、form-C deny 不可被 Unsupported 绕过、拒绝点名到段

- 文件面: `crates/kanzei-harness/src/permission.rs`
- 改动: 只改一个文件(F0 已把字段加好、F1 已让 value 可解析、F2 已给出词法器)。
1) 形态判据 `Rule::kind()`:`command.is_some()` → `Kind::Segment`,且**要求 `resource` 为空**,否则算配置错误(不参与匹配 + 由 F8 产告警,fail-closed)。三档**结构上互不相交** —— 形态 A/B 的规则永远不进段级路径,红线「旧纯字符串规则不得授权结构化请求」由**结构隔离**保住,不靠比较逻辑。
2) 新增 `pub fn explain(&self, action, resource) -> Decision { effect, matched: Option<Rule>, reason: Option<String> }`;`evaluate` 改为 `explain(..).effect`,**签名不变,6 个既有调用方零改动**。
3) **bash 判定顺序(每一步对应一条红线,顺序不可换)**:
  ⓪ `hard_denies` 全表扫描(用整串 resource,与今天同一个 `resource_match_for_action`)→ Deny。保住 readonly 的 `*` 硬 deny 与不可覆盖性。
  ① value 不是结构化 JSON(`"*"` 探针、`action_fully_denied`)→ **直接走今天的旧路径,逐字节不变**。
  ② 形态 Segment 的 **deny** 先判:`scan` 得 Segments 时任一段命中 → Deny,reason 点名该段;`scan` 得 `Unsupported` 且**存在任何一条 workdir 匹配的 form-C deny 规则**时 → **Ask**(不落穿)。这一步是证伪一 concerns 第 1 条的处置:两份原方案在 Unsupported 时都直接落回旧路径,若配置里有 yolo 就变成 Allow,「写一条 form-C deny 收紧 yolo」给的是假安全感。代价必须写进注释与告警:**一旦写下任何 form-C deny,所有不可静态判定的命令都会回到 Ask,即使存在 `resource="*"` 的整体放行** —— 这是用户显式写下 deny 换来的,不是隐式提严。
  ③ 旧路径(形态 A/B,含 `command_chaining_escapes`)跑一遍:Allow → **直接 Allow,不做任何分段校验**(保住 :465 的「显式 `*` yolo 不降级」与 :479 的「精确规则可放行含 `rm -rf ~` 的整串」);Deny → Deny。
  ④ 只有 ③ 得 Ask 才进段级 allow 判定。**这一步是「缺省行为不变」的机械论证:新档只能把 Ask 变成 Allow/Deny,不可能改变今天任何一个 Allow 或 Deny。**
4) `segment_allow(command, workdir)`:
  a. `cmdline::scan`;`Unsupported(c)` → Ask,reason = `不可静态判定的构造:{c};请拆成不含该构造的单条命令`。
  b. 每段先过 `ESCAPE_TOKENS` 与 `RELOCATION_FLAGS`(F2 的两张表,大小写不敏感)→ Ask,reason 点名该段与该 token。
  c. workdir 维度:`rule.workdir` **必填**;`"*"` = 任意,但**必须用户显式写出来**(D-267 修复方向原文:「规则能写任意 workdir,但必须是用户显式写出来的,不是旧规则被默认提权」);否则与实际 workdir 走 `resource_match`(路径语义,双侧 normalize)。第一版只接受 `"*"` 或绝对路径 glob,相对路径视为配置错误并告警(Ruleset 没有项目上下文,不给它塞根)。
  d. command 维度:pattern 与段各自 `cmdline::tokens` 后**逐 token** `wildcard_match`,pattern 末尾单独的 `*` 吃掉剩余全部 token(逐 token 而非整段通配,避免 `cargo *` 命中 `cargotest`)。任一侧 tokens 为 None → Ask。
  e. 段级 last-match-wins;**每一段都 Allow** 才整体 Allow;否则 Ask,reason 点名**第一个未获授权的段**(D-267 实测清单 (b) 条:拦截必须点名是哪一段,否则模型无法自我修正)。
5) `denial_hint` 加 bash 分支:命中托管族时保持 D-173 原文案不变;否则把 `explain().reason` 拼进去。
- 测试:
  - `可手写可复用的任意workdir规则`(验收①):`Rule::command("bash","cargo *","*",Allow)` 对 `{"command":"cargo test --all","workdir":"c:/anywhere"}` → Allow
  - `无链接符不再降级_有链接符仍按段判定`(验收②两个方向):同一条 `cargo *` 下 `cargo test` → Allow;`cargo test; rm -rf ~` → Ask 且 reason 含 `rm -rf ~`;`cargo build && cargo test` → Allow
  - `旧纯字符串规则不进段级路径`(结构隔离的反证):只有 `Rule::exact("bash","cargo *",Allow)` 时结构化请求仍 Ask
  - **`workdir是授权身份的语义级反证`**(证伪一 FATAL-3 与 missing 第 2 条的直接落点,今天全仓没有这一组):在 `command="git *" workdir="c:/main"` 与 `command="cargo *" workdir="c:/main"` 下,断言 `git -C ../other reset --hard`、`git --git-dir=../o/.git --work-tree=../o clean -fdx`、`cargo --manifest-path ../o/Cargo.toml test`、`cargo build --config C:/x.toml`、`cargo build --target-dir ../o` 全部 Ask 且 reason 点名重定位 flag。大小写变体各一条
  - `D-051四反例在新形态下同样不放行`:`command="*" workdir="*"` 下 `git status > x.md`(redirection)、`git -c alias.x=!calc x`、`python -c open_secret`、`pwsh -Command Set-Content secret x` 全部 Ask 且 reason 点名构造或 token
  - `form_C_deny在段级生效且不可被Unsupported绕过`:`command="rm *" workdir="*"` deny 与 `resource="*"` allow(yolo)并存 → `cargo build; rm -rf ~` → Deny;`cargo build > out.txt` → **Ask**(不是 Allow;证伪一 concerns 第 1 条的机械守卫)
  - `硬deny不可被新形态覆盖`:`push_hard_deny(bash,"*")` + form-C allow → Deny;`action_fully_denied("bash")` 仍为 true
  - `非法form_C规则不授权`:缺 workdir / action != bash / 同时写了 resource / workdir 写相对路径,四种各一条 → 仍 Ask
  - `探针与action_fully_denied不受影响`:无 form-C 规则时 `evaluate("bash","*")` 与结构化探针的结果与改前逐字节相同
  - 既有 :442/:465/:472/:476/:479/:494/:517/:549/:590/:601 **一字不改**保持绿
  - F1 的 `gate守卫` 与 `向后兼容12条` 保持绿
- 验收: ① D-267 验收①②③④ 各有 ≥1 条命名可与验收原文对上的测试;② `git diff crates/kanzei-harness/src/permission.rs` 在 :441-515 区间(D-051 四条测试)**零删除行、零修改行**(只允许在区间外新增) —— 这条取代两份原方案里「保留 vs 逐条改写」自相矛盾的两句话:D-267 验收③ 的原文是「保留且仍绿」,验收② 由**新形态**的独立测试满足,旧形态一个字符不动;③ `cargo test -p kanzei-harness permission` 全绿;④ `cargo clippy --workspace --all-targets -- -D warnings` 全绿(F0 已加宽字段,本批只在 permission.rs 内动)。

### F6 · R-177 内容②(验收②)+ 证伪三 FATAL-3 的处置 · cwd 真正指向 worktree,并同批修好一落地就暴露的三处(后台围栏 / frontend / files)

- 文件面: `crates/kanzei-app/src/run.rs`, `crates/kanzei-tools/src/bash.rs`, `crates/kanzei-tools/src/frontend.rs`, `crates/kanzei-tools/src/files.rs`, `crates/kanzei-app/src/process_tests.rs`
- 改动: 1) run.rs:1672 归属校验 `process.project_dir != project_root` → 比 `process.origin_project`(F4 定死 project_dir 恒主根后两值恒等,行为不变;改的是意图,R-177 内容②)。
2) run.rs:1723 传给 `run_task` 的 `project_dir` 改为 `process.worktree_path.clone().unwrap_or(project_dir.clone())` —— 这是**唯一**让 cwd 真正指向 worktree 的地方;`main_root` 参数不动。
3) run_task 内把 `let project_root = main_root;`(:70)提到配置加载(:64)之前,并把 `KanzeiConfig::load_with_warnings(&cwd)` 改成 F3 提供的 `load_with_warnings_at_root(&project_root)`(R-177 内容⑧ / 验收③)。
4) run.rs 轮末 `memory::consolidate_memory_inbox(project_dir.clone())` 改传 `main_root`(运行时线路径上仅存的发现式取根;传主根后 memory 内部的 discover 是恒等)。
5) **同批必修的三处潜伏面**(证伪三 FATAL-3:它们今天不在任何条目的内容字段里,属无人认领;判定依据是**验收**而不是根因 —— 验收② 要求「线内写代码落在 worktree」,而这三处会让线读到另一棵树):
  - bash.rs `background_workdir_breach` 的包含根从 `ctx.project_root` 改 `ctx.cwd`,`.kanzei` 排除同时检查 `ctx.cwd/.kanzei` 与 `ctx.project_root/.kanzei`。**不改则线里所有 `background:true` 的 bash 被无条件拒**。D-174 的两条语义(不得跑出可归因范围、不得扎进托管树)完整保留,只是「可归因范围」正名为代码树;既有 D-174 测试**改写而非删除**。
  - frontend.rs:181/242 的 `ctx.project_root.join(rel)` → `ctx.cwd.join(rel)`;files.rs:382/386 的 `scan(&ctx.project_root)` / `load_annotations(&ctx.project_root)` → `ctx.cwd`。实测复核:`read`/`glob`/`grep`/`write`/`edit` 已经走 cwd,不改这两个工具会让同一个 agent **在两棵树之间读写** —— frontend 读主树内容、edit 按 cwd 落 worktree,old_string 匹配失败或基于陈旧上下文改对了地方,正是 R-185 点名的「git 一字不报的语义撞车」。
  - **判据统一并写进注释**:凡 `.kanzei/**` 资产走 `project_root`,凡仓库源码走 `cwd`。
6) `ManagedSnapshot::capture(&ctx.project_root)` **保持取主根不变**(托管文档只有主根一份,这是对的),但补一条测试固化「worktree 内的 `.kanzei` 副本不在围栏辖区」这一事实,免得后来人以为是漏了。
7) `git_batches::commit_subjects` 的取根:本批只做**定性并加注释**(按同一判据应取 cwd 的 git),不改行为 —— 勘察未能在本轮确认它的实际形态与消费链影响,不做没读透的改动;写进条目进展并单开缺陷(见 not_doing)。
- 测试:
  - run.rs `线上运行cwd是worktree_project_root是主根`:构造带 worktree_path 的 handle,断言传给 run_task 的两个路径**不相等**且各自正确(验收②前半)
  - run.rs `配置从主根加载_worktree副本改了不生效`:两处 kanzei.toml 写不同 default_profile,断言取主根那份(验收③)
  - bash.rs `后台workdir以代码树为界`:cwd=worktree、project_root=主根时 workdir=worktree 子目录**不**被拒;workdir=`worktree/.kanzei` 与 workdir=主根(树外)仍被拒;既有 D-174 测试相应改写
  - bash.rs `托管快照仍取主根_worktree内kanzei副本不在辖区`(固化事实,防误判为遗漏)
  - frontend.rs / files.rs `读的是本线的树`:worktree 里改一个文件,断言两个工具看到的是改后的内容;主树同名文件不同内容时断言**没有**读到主树那份
  - process_tests.rs `线的session_id仍由主根算出`(D-176 红线,与 F4 同源再确认一次)
  - tool.rs:274/302/316 三条双键测试保持绿(锁键按树分开、写仲裁键按主根收敛)
- 验收: ① `grep -n 'load_with_warnings(&cwd)' crates/kanzei-app/src/run.rs` 零命中;② `grep -n 'process.project_dir != ' crates/kanzei-app/src/run.rs` 零命中;③ `grep -n 'ctx.project_root' crates/kanzei-tools/src/frontend.rs crates/kanzei-tools/src/files.rs` 零命中;④ `cargo test -p kanzei-app -p kanzei-tools -p kanzei-core -p kanzei-harness` 全绿;⑤ fmt/clippy(`-p kanzei-app -p kanzei-tools --all-targets -- -D warnings`)全绿。

### F7 · R-182 内容②④ / 验收⑤ · CLI 双键拆开(worktree_key=cwd / write_key=主根),工具侧配置改读主根

- 文件面: `crates/kanzei/src/main.rs`, `crates/kanzei-tools/src/webfetch.rs`, `crates/kanzei-tools/src/websearch.rs`, `crates/kanzei/tests/worktree_main_root.rs`
- 改动: 1) main.rs 的 `with_identity`(:186-197):第一参数 worktree_key 从 `project_root.display()` 改为 `cwd.display()`,第二参数 project_write_key 保持规范化主根。今天两参同值;worktree 上线后不改会让同项目 N 棵树共用一把工具锁互相串死 —— tool.rs:274 的语义在 CLI 侧才第一次成立。注释里删掉那句已过时的「CLI 是单工作树,代码树即项目根,两把键同源」。
2) webfetch.rs:53 与 websearch.rs:62 的 `KanzeiConfig::load(&ctx.cwd)` → `load_at_root(&ctx.project_root)`(F3 提供)。代理配置是主根资产,从 worktree 跑时不能读分支副本(R-182 内容④)。注释写明与 F6 同一条判据。
- 测试:
  - main.rs `cli双键在worktree下必须分叉`:构造 ctx 后断言 `worktree_concurrency_key()` 含 worktree 路径、`project_write_key()` 是主根,两者**不等**(R-182 验收⑤)
  - kanzei-tools `webfetch与websearch取代理配置用主根`:主根与 cwd 各放一份 kanzei.toml(proxy 值不同),断言取到主根那份
  - worktree_main_root.rs 追加 `worktree内跑kz时project_root不等于cwd且state.db落主根`
- 验收: ① `grep -n 'load(&ctx.cwd)' crates/kanzei-tools/src` 零命中;② 双键分叉测试存在且绿;③ `cargo test -p kanzei -p kanzei-tools -p kanzei-harness` 全绿;④ `cargo fmt --all -- --check` + `cargo clippy -p kanzei -p kanzei-tools --all-targets -- -D warnings` 无输出。

### F8 · D-267 验收⑥ + R-183 验收② + 证伪二的诚实处置 · 启动告警说实话:yolo 判据修正、ACE 类程序告警、worktree workdir 失配点名、非交互策略键三处接线

- 文件面: `crates/kanzei-harness/src/config.rs`
- 改动: 只改一个文件,全部是配置层的告警与 schema。
1) **yolo 文案判据修正**(D-139 以新形态复发的必修点,方案A 漏了):`bash_permission_warnings` 今天以「探针 `{"command":"git status","workdir":"."}` 评估为 Allow」推出 yolo。有了 form C 之后探针 Allow 可能只是因为用户写了 `command="git *" workdir="*"`。改成:探针 Allow **且** `explicit_bash_wildcard_allows().len() > 0` 才说「全量放行(yolo)」;探针 Allow 但无 `*` 规则 → 说「常见只读命令已由 N 条 command 规则覆盖,其余仍会询问」。
2) `explicit_bash_wildcard_allows` 补两类今天认不出的隐藏 yolo:`action == "*" && resource == "*"` 的规则;form-C 的 `command == "*" && workdir == "*" && effect == Allow`。
3) **ACE 类程序告警(证伪二 FATAL-1/2 的处置)**:对每条 form-C allow 规则,取 command pattern 的首 token,若命中 `cmdline::ACE_PROGRAMS`(cargo/node/npm/npx/pnpm/yarn/python/python3/pwsh/powershell/sh/bash/awk/perl/ruby/php/deno/bun/make/go/dotnet/java/uv/pip/rustc)则产一条告警,原文必须说清:「该规则等于在 `<workdir>` 授予**任意代码执行** —— 这些程序按设计会编译并运行工作树里的代码(build.rs、测试二进制、脚本)。段级闸门只保证**被调起的程序与工作目录**是你写下的那一组,不保证被调起的程序自身不能执行任意代码。」`workdir == "*"` 时告警升级为更强的措辞并点名「范围未受限」。**不阻断,只告知**(D-004 口径:任何不做的理由都要说出来)。
4) legacy 文案给出**可照抄的收敛路径**(D-267 验收⑥):「N 条一次性 bash 规则各只覆盖一个命令,可合并为 `[[permissions.rules]] action="bash" command="cargo *" workdir="c:/你的主根" effect="allow"`」。**示例里写具体主根路径而不是 `workdir="*"`** —— 两份原方案的模板与文案都示范 `workdir="*"`,那会让「workdir 是授权身份的一部分」在推荐配置下只剩类型层面的存在(证伪一 missing 第 5 条)。`"*"` 保留为用户可以显式选择的写法,但不由我们推荐。
5) **worktree workdir 失配告警(R-183 验收③ 的可见性半边,两份方案零覆盖)**:新增 `pub fn bash_rules_pinned_elsewhere(&self, cwd: &Path) -> Vec<&Rule>` —— 列出 workdir 被钉死在 **cwd 之外**的 bash 规则(旧结构化形态里解析出的 workdir,以及 form-C 的具体 workdir)。当该列表非空时产告警:「检测到 N 条 bash 规则的 workdir 指向 `<主根>`,而本次运行的工作目录是 `<worktree>`;这些规则本次一条都不会命中。要让它们在任意工作树生效,请改写成 `command`/`workdir` 形态并显式写出 workdir 的作用范围。」实测复核:当前配置 12 条结构化规则的 workdir 全部硬编码 `c:/users/kanzei/documents/kanzei code`,而 `resources_with_ctx` 用 `ctx.cwd.join(workdir)` 造资源 —— 这就是线一启动就等于空白允许清单的成因,今天完全无声。
6) **非交互策略键**:`PermissionsSection` 加 `#[serde(default)] pub non_interactive: Option<String>`(留成 String 而非枚举:未知取值不能炸启动,只能 fail-closed 回落 deny)。新增 `NonInteractive { Deny(default), RulesOnly, AllowListed }` 与 `non_interactive_policy()`。**三处接线一个不能漏**(barrier_timeout_secs 的前车之鉴):`unknown_keys` 的 permissions 已知键清单(今天只有 `["rules"]`)、`merge()` 的 permissions 段(今天只有 `rules.extend` 一行,补标量覆盖)、告警(解析失败产一条 fail-closed 告警)。
7) 新增 `pub fn permission_rule_warnings(&self) -> Vec<String>`:对每条非法 form-C 规则(缺 workdir / action != bash / 同时写了 resource / workdir 写成相对路径)各产一条中文告警,明说该规则**不会授权任何东西**。
- 测试:
  - `告警与实际评估一致`(D-139 加固,四组夹具):只有 form-C 规则 / 有显式 bash `*` / 有 `action="*" resource="*"` / 有 form-C 的 `command="*" workdir="*"` —— 四种文案各不相同且都如实,尤其**form-C 非整体放行时不得误报 yolo**
  - `ACE类程序规则产出任意代码执行告警`:`command="cargo *"` 与 `command="ls *"` 两条,断言前者有告警且文本含「任意代码执行」、后者没有;`workdir="*"` 时断言文案含「范围未受限」
  - `旧bash规则告警给出可照抄的收敛写法`:断言文本同时含 `command` 与 `workdir` 两个键名,且**不含** `workdir = "*"` 子串(推荐语不得示范无限范围)
  - `worktree下workdir失配的规则被点名`:12 条真实规则做成 fixture,cwd 传一个 worktree 路径 → 断言告警条数与规则条数对得上、文本同时含主根与 worktree 两个路径
  - `非交互策略缺键等于deny_旧配置行为不变` / `非法取值fail_closed回落deny并告警` / `项目层可覆盖全局层`(merge overlay 的守卫)/ `新键不产生未知配置项假告警`(unknown_keys 的守卫)—— 四条,对应「三处接线」的每一处
  - `非法form_C规则在启动告警里可见`:四种非法形态各一条
- 验收: ① 上述七组测试全绿,`cargo test -p kanzei-harness config` 通过;② `grep -n 'workdir = \\"\*\\"' crates/kanzei-harness/src/config.rs` 在告警文案字符串里零命中(推荐语不示范无限范围);③ 三处接线各有一条命名测试(不靠人工核对);④ **不做**「用改后二进制人工跑一遍比对文案」这类不可机械核验的验收 —— 全部由 fixture 断言承担;⑤ `cargo fmt --all -- --check` + `cargo clippy -p kanzei-harness --all-targets -- -D warnings` 无输出。

### F9 · R-177 内容③ / 验收④ · 线清单真源改 git worktree list --porcelain,废除 localStorage,两段冒烟等价重写并带变异守卫

- 文件面: `crates/kanzei-tools/src/git.rs`, `crates/kanzei-tools/src/lib.rs`, `crates/kanzei-app/src/processes.rs`, `crates/kanzei-app/src/main.rs`, `crates/kanzei-app/ui/09-sessions.js`, `scripts/ui-runtime-smoke.mjs`
- 改动: 1) git.rs 新增 `pub struct WorktreeEntry { path, branch: Option<String>, bare, detached, locked, prunable }` 与 `pub fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry>`;既有私有 `worktree_for_branch` 改成基于它的三行实现,**签名与返回类型不变**,`merge_ff` 调用点不动。lib.rs 导出这两个符号。
2) processes.rs 新增 Tauri 命令 `worktree_list(project_dir) -> Vec<WorktreeInfo>`:在主根跑 `git worktree list --porcelain` → parse → 逐条补 status/diff(复用 worktree_diff 的逻辑),主根自身那条剔除,并合并 ProcessHandle 的绑定关系;main.rs 的 invoke_handler 注册。
3) `validate_worktree_path` 的判据从「必须位于项目父目录之下」改为「必须出现在 `git worktree list --porcelain` 的输出里且不等于主根」——既堵住路径逃逸(更严:兄弟目录里的另一个项目根不再通过),又满足验收④「手工 `git worktree add` 出来的树也能被发现」。
4) 09-sessions.js:`refreshWorktrees` 改调 `worktree_list`(仍在 await 前认领 forProject);删除五处 `localStorage["kz-worktrees:*"]` 读写与 `#worktree-add` 里的清单维护;`handleWorktreeAction` 的 discard 分支不再改清单,只重刷。
5) ui-runtime-smoke.mjs 的 D-251 段与 D-257 段**等价重写而非删除**:被守护的性质分别是「切项目时清单不错位」与「`#worktrees-refresh` 真的绑了监听器」。新形态断言:`worktree_list` 的 projectDir 入参在 await 前认领、切走后不把甲项目的返回画进乙项目面板;点击刷新后真打出 `worktree_list` IPC 且带正确 project_dir;写入去向探针改成「不得再出现任何 kz-worktrees 键写入」。
6) **给这两条断言各接一个变异开关**:`KZ_SMOKE_MUTATE=d251` / `=d257` 时故意破坏被守护的行为。这是把两份原方案里「人工验证一次删掉任一条即变红」换成机械判据(证伪三 concerns 第 4 条)。
- 测试:
  - git.rs `parse_worktree_list识别分支_bare_detached_locked_prunable`:表驱动,覆盖 `--porcelain` 的全部行形态(该解析器今天零直接单测,抽出即补上)
  - git.rs `merge_ff_fast_forwards_branch_checked_out_in_linked_worktree` 保持绿(抽取时唯一会红的地方,发版流程直接依赖)
  - worktree_tests.rs `手工建的worktree也能被发现`:直接 `git worktree add` 不经 process_create,断言 `worktree_list` 返回它(验收④)
  - worktree_tests.rs `validate_worktree_path只认git认得的树`:兄弟目录里的另一个 git 仓被拒
  - ui-runtime-smoke.mjs:切项目清单不错位、刷新按钮真绑并打出 worktree_list IPC、无任何 kz-worktrees 写入,三条
- 验收: ① `grep -rn 'kz-worktrees' crates scripts` 零命中(验收④);② `node --check` 全量 + `node scripts/ui-runtime-smoke.mjs` 退出 0;③ **变异守卫三次调用写进同一条验收**:`KZ_SMOKE_MUTATE=d251 node scripts/ui-runtime-smoke.mjs` 与 `KZ_SMOKE_MUTATE=d257 ...` 各期望**非零**退出码,正常跑期望 0 —— 前端改动不得只以 `node --check` 为证据(conventions §1.3);④ `cargo test -p kanzei-tools -p kanzei-app` 全绿;⑤ fmt/clippy(`-p kanzei-tools -p kanzei-app --all-targets -- -D warnings`)全绿。

### F10 · R-183 内容① / 验收②⑤ · 非交互三态:AskPolicy 落在 RunnerConfig,rules-only 走 Gate::Deny 回喂模型并继续

- 文件面: `crates/kanzei-core/src/runner/mod.rs`, `crates/kanzei-core/src/runner/drive.rs`, `crates/kanzei/src/main.rs`
- 改动: 1) core:`RunnerConfig` 加 `pub ask_policy: AskPolicy`(`Interactive` | `RulesOnly`),`Default` 即 `Interactive` —— **桌面端与全部既有测试零改动、逐字节不变**,「桌面端不受非交互策略影响」由类型层面保证而不是靠纪律。
2) drive.rs 两个站点消费它:
  - 串行门禁 :811 处:`RulesOnly` 时**不调 ask**,直接 `gate_result = Gate::Deny(resource)` 并落 `PermissionResolved{decision:"denied", source:"noninteractive"}`。**这是三态能成立的关键语义区分**:`Gate::Deny` 产出 `permission denied by ruleset: … + denial_hint`(F5 已让 hint 对 bash 点名具体段/构造)回喂模型并**继续本轮**,而 `AskReply::Deny` 走 `Gate::UserDeclined` 停整轮 exit 3 —— 后者与今天 EOF 逐字节同义,做成那样 rules-only 与 deny 在代码上无法区分。
  - `can_parallel_tools` 预检 :546 处:`RulesOnly` 时 `Ask` **不再置 ready=false**(它不会阻塞、结果确定),并发 wave 得以保留;否则每批工具都退回串行 ask 路径,与 R-182 验收③ 反向。
3) main.rs 新增**纯函数** `fn ask_policy(is_tty: bool, policy: NonInteractive, env_override: Option<&str>) -> AskPolicy`:`is_tty == true` → 恒 `Interactive`(唯一开关,有 TTY 时代码路径与今天逐字节相同);非交互 + Deny → `Interactive`(= 今天 EOF→Deny 停机,行为不变)+ 多打一行 stderr 说明是「非交互 + 策略 deny 导致停机,可在配置里改 `non_interactive`」;非交互 + RulesOnly/AllowListed → `RulesOnly`。`KANZEI_NONINTERACTIVE` 环境变量可覆盖配置(脚手架派发用)。调用处 `use std::io::IsTerminal; std::io::stdin().is_terminal()`(std 自带,零新依赖)。
4) **ask 闭包(main.rs:394-417 的 Permission 分支)一个字符都不改**;Question 分支同样不动(取消不得升级为自动作答)。
- 测试:
  - main.rs `ask_policy决策表`:2×3 六格全断言,尤其 `is_tty=true` 三格全部 Interactive(R-183 验收⑤:检测本身有测试,不靠读到 EOF 倒推)
  - main.rs `缺省仍是deny且旧配置行为不变`:无 `non_interactive` 键 + 无 TTY → 仍走今天的 EOF→Deny 路径,exit 3 断言保持绿
  - drive.rs `rules_only下未授权工具被拒但本轮继续`:ask 恒不被调用,断言 `RunSummary.halted_by_user == false`、该工具结果 is_error 且文本含 denial_hint
  - drive.rs `deny档仍停整轮`:对照组,`halted_by_user == true`
  - drive.rs `rules_only不阻断并行wave`:两个 Ask 资源的工具批仍走并行路径(读事件序列断言)
  - `子代理ask恒Deny不受策略影响`(红线):subagent.rs 路径断言
  - 既有 `declined_tool_batch_keeps_real_and_placeholder_results_paired`(D-054 成对落库)与 always_allow_bash.rs 的三条保持绿
- 验收: ① `git diff crates/kanzei/src/main.rs` 中 ask 闭包的 Permission 分支**零改动行**;② `grep -n 'is_terminal' crates/kanzei/src/main.rs` 恰一处且被纯函数包裹(测试不直接调 stdin);③ `grep -rn 'AskPolicy' crates/kanzei-app` 零命中(桌面端不受影响的机械证据);④ 六格决策表测试与 rules-only/deny 对照测试全绿;⑤ `cargo test -p kanzei -p kanzei-core -p kanzei-harness` 全绿;⑥ fmt/clippy(`-p kanzei -p kanzei-core --all-targets -- -D warnings`)全绿。

### F11 · R-177 内容⑦ / 验收⑧ · N3 开关:线默认不写主根 tracker,拒绝要点名理由而不是静默失败

- 文件面: `crates/kanzei-harness/src/permission.rs`, `crates/kanzei-app/src/state.rs`, `crates/kanzei-core/src/store/processes.rs`, `crates/kanzei-core/src/store/schema.rs`, `crates/kanzei-app/src/run.rs`, `crates/kanzei-app/src/process_tests.rs`
- 改动: 1) permission.rs 新增 `pub fn push_denial_note(&mut self, rule: Rule, note: &str)`:内部复用 `ManagedResource` 容器(`required_tool: None` + note),让**普通 Deny 规则**也能带一句可回喂的理由;`denial_hint` 已经查 `managed_for`,零改动即生效。**选普通 Deny 而不是硬 deny**:硬 deny 会走 `action_fully_denied` 把工具整体摘出快照,模型根本看不见,等于静默失败;验收⑧ 要求「明确拒绝并说明原因,不是静默失败」。
2) `ProcessHandle` 加 `tracker_writes_enabled: Arc<AtomicBool>`(默认 false);`StoredProcess` 加同名列 + schema 迁移一列(可空 INTEGER,缺省 0),`upsert/list/get` 三条 SQL 同步;`process_create/update` 增可选参数;`ProcessInfo` 回显;同时把线的**分支名**在 `ProcessInfo` 里回显(替代被砍掉的 session_id 后缀,见 not_doing)。
3) run.rs:线(`worktree_path.is_some()`)且开关关时,在组件链里(ConfigComponent 之后)push 六条普通 Deny 规则(req/defect/goal/decision/source/finding 各 `resource="*"`)+ `push_denial_note`:「本条线只写代码;需求/缺陷等托管文档由主树统一登记(可在线设置里打开『允许写 tracker』)」。主树进程(worktree_path == None)一条规则都不推,行为逐字节不变。
4) **把 N3 的原始理由已失效如实记进 R-177 进展**:原始理由是「线里的 agent 一调 tracker 就被写租约卡几分钟」,R-182 把强制口径降为单次操作文件锁后这条理由消失。默认值仍保留为关(用户定案未改),但事实要写下来供用户下次拍板。
- 测试:
  - permission.rs `push_denial_note让普通deny也能给出理由`:断言 `denial_hint` 返回 note 文本,且该规则**不**进 hard_denies(`action_fully_denied` 仍为 false,工具不被摘除)
  - permission.rs `managed_hard_deny_carries_its_legal_alternative`(:549)保持绿
  - worktree_tests/run.rs `线默认拒绝写tracker且理由可读`:构造线 harness,断言 `evaluate("defect","*") == Deny` 且 denial_hint 含「主树统一登记」(验收⑧前半)
  - `开关打开后两条线并发写主根tracker编号互异且条目全部存活`:这是验收⑧后半「走写租约排队」的**可判定重述** —— R-182 正在撤 run 级租约,断言租约存在会与之撞车;实际守的性质是「由 `atomic_file::FileLock` 兜住」,用两个真 OS 进程验(证伪三 concerns 第 13 条的处置)
  - process_tests.rs `开关默认关且落库回填` + `旧库无该列时读出默认关`(schema 迁移向后兼容)
  - process_tests.rs `process_sessions_are_isolated_but_default_keeps_legacy_id` 保持绿
- 验收: ① 验收⑧ 前后两半各有一条命名测试;② schema 版本号 +1 且注释里带迁移与**回滚**说明(conventions §4 M2);③ **迁移可判定**:一条测试用不含新列的旧 schema 建库 → `list_processes` 成功且开关读出 false;一条测试新建库 → 列存在、默认 0;并断言 schema_version 恰好 +1(取代两份原方案里「两种启动都能跑」这类无判据的话);④ `cargo test -p kanzei-app -p kanzei-core -p kanzei-harness` 全绿;⑤ fmt/clippy 全绿。

### F12 · R-183 内容③④ / 验收③④ · per-run allowlist(workdir 由 CLI 钉成本次 cwd)+ 自动放行的可查轨迹与退出汇总

- 文件面: `crates/kanzei/src/main.rs`, `crates/kanzei-harness/src/permission.rs`, `crates/kanzei-core/src/runner/event.rs`, `crates/kanzei-core/src/runner/drive.rs`, `crates/kanzei-app/src/run.rs`, `crates/kanzei-app/src/state.rs`
- 改动: 1) config 侧已在 F8 备好键。main.rs 新增 `load_allowlist(path) -> Vec<Rule>`:用既有 serde 类型解析一个与 kanzei.toml **完全同语法**的文件(不发明第二套语法)。
2) **allowlist 里的 `workdir` 由 CLI 钉成本次运行的 cwd**,条目只需写 `command`;条目可以显式写 `workdir = "*"` 来 opt-out,但那必须是操作员亲手写下的。这是两份原方案里最重要的一处合并改进:方案A 的模板要求每条 `workdir = "*"`(把「workdir 是身份」架空),方案B 的 B6 已经是「恒为本次 cwd」——取方案B 的,并补上 opt-out 通道。它同时是 **R-183 验收③ 的可满足半边**:同一份 allowlist 从主根跑与从 worktree 跑都能命中,因为 workdir 由运行时注入而不是写死在文件里。
3) 新增 CLI 专属组件 `AllowlistComponent(Vec<Rule>)`,`contribute` 只做 `draft.permissions.extend(...)`;装配链里加在 `ConfigComponent` **之后**(last-match-wins 让本次运行的清单赢过项目配置,但**赢不过硬 deny**)。放在 Ruleset 而不是 ask 闭包里,是为了让 `drive.rs:521-560` 的并行预检看得见它。
4) 策略 == AllowListed 时必须能读到 `KANZEI_ALLOWLIST` 指向的文件,缺失/不可读/解析失败一律 `bail!` **在开跑前**(fail-closed,不静默降级成空清单);策略 != AllowListed 时即使设了该变量也**不加载**(避免「设了变量以为生效」的半覆盖,D-187 教训)。
5) permission.rs 新增 `pub fn matched_rule_text(&self, action, resource) -> Option<String>`:复用 F5 的 `explain().matched`,返回人读原文(form C 给 `bash command="cargo *" workdir="…" effect=allow`,旧形态给 `bash resource="…" effect=allow`)。**不改 evaluate 的签名与任何既有调用方**。
6) `RunEvent::PermissionResolved` 加 `rule: Option<String>` 与 `reason: Option<String>`;桌面端 run.rs/state.rs 两个消费方同步(conventions §4:权限契约变更必须同步 CLI 与桌面端)。
7) main.rs 非交互模式下把每条 PermissionResolved 经既有 `store.append_event(session_id, "permission.resolved", payload)` 落 state.db(**复用既有能力,不新建 sink**),并在退出前打一张 stderr 汇总表(自动放行 N 条 / 非交互拒绝 M 条,逐条列 action、resource、命中规则原文、拒绝理由)。交互模式下不打,行为不变。
- 测试:
  - `allowlist与kanzei_toml同语法`:同一段 TOML 分别作为项目配置与 allowlist 解析,得到等价 Rule 向量
  - `allowlist的workdir由本次cwd注入`(R-183 验收③ 的机械形态):同一份只写 `command` 的 allowlist,分别以 cwd=主根 与 cwd=worktree 装配,断言两次都命中对应 workdir 的资源;并断言**没有** opt-out 的条目不会命中另一个 workdir(红线:workdir 是身份的一部分)
  - `allow_listed缺文件必须开跑前报错` 与 `非allow_listed时不加载allowlist`
  - `allowlist赢不过硬deny`:allowlist 写 `action="write" resource="*" allow`,断言 `.kanzei/project/requirements.md` 仍是 Deny
  - `matched_rule_text给出新旧两种形态的原文` + `matched_rule_text与evaluate同源`(随机若干组 (rule, value),断言 is_some 当且仅当 evaluate 不是「无匹配」)
  - crates/kanzei/tests/noninteractive_run.rs:allowlist 含 `command="cargo --version"`,mock SSE 让模型发该命令,断言自动放行、进程跑完 exit 0;跑完后断言 stderr 汇总含规则原文、state.db 里有 `permission.resolved` 事件且 payload 的 rule/reason 非空(验收④)
  - `交互模式下不打汇总`:同一测试在 is_tty=true 分支断言 stderr 无新增输出
- 验收: ① R-183 验收③④ 各有命名可与验收原文对上的测试;② `grep -rn 'KANZEI_ALLOWLIST' crates/kanzei-app` 零命中(桌面端不受影响);③ allowlist 文件解析后每条的 workdir 非空(由注入保证,不靠文件里写);④ `cargo test -p kanzei -p kanzei-harness -p kanzei-core -p kanzei-app` 全绿;⑤ `cargo fmt --all -- --check` + `cargo clippy -p kanzei -p kanzei-core -p kanzei-harness -p kanzei-app --all-targets -- -D warnings` 无输出。

### F13 · R-177 内容⑤ / 验收②⑥⑦ · 四个 worktree 命令补测试 + 线的端到端闭环(.kanzei 副本字节级零改动)

- 文件面: `crates/kanzei-app/src/worktree_tests.rs`, `crates/kanzei-app/src/process_tests.rs`
- 改动: 纯测试批,零生产代码改动(F4 已补 create、F9 已补 list 与 validate)。
1) `worktree_diff` / `worktree_merge` / `worktree_discard` 各补测试,用真 git 夹具:diff 返回未提交改动与真实 diff 文本;merge 走 `merge-tree --write-tree` 预检 —— 分别构造「可干净合并」与「必冲突」两种,后者断言错误文本保留双方改动且**未执行** `git merge`;干净时断言以 `--no-ff` 落地(N2 定案的机械守卫);discard 在有未提交改动时失败且提示「已保留以便恢复」。
2) **测试一律断言命令的可观察结果,不断言「取没取写租约」** —— R-182 会把强制口径从 run 级租约降为单次操作文件锁,断言租约行为会与之撞车。
3) 端到端(验收②的完整形态):建线 → 在 worktree 里写一个源码文件并提交到线分支 → 断言 ①主树该文件未变 ②`<worktree>/.kanzei` 下**全部文件**的 sha256 与建树时逐字节相同 ③tracker/state.db/记忆全部落主根 ④删树后该线的会话历史仍可从主根 state.db 读回(验收⑥)。
- 测试:
  - `worktree_diff返回真实改动与diff`
  - `worktree_merge冲突时不执行合并且保留双方`
  - `worktree_merge干净时以no_ff落地`
  - `worktree_discard有未提交改动时保留现场`
  - `线上闭环_主树零改动_worktree内kanzei副本哈希不变`(验收②)
  - `删树后线的会话历史仍可回放`(验收⑥)
- 验收: ① `cargo test -p kanzei-app worktree -- --list` 的输出里 `worktree_create` / `worktree_diff` / `worktree_merge` / `worktree_discard` 四个命令名**各出现 ≥1 次**,写成四条独立断言(取代两份原方案里判不出验收⑦ 的 `grep -c 'fn worktree_'`);② 哈希比对测试存在且绿;③ `cargo test -p kanzei-app` 全绿;④ fmt/clippy(`-p kanzei-app --all-targets -- -D warnings`)全绿。

### F14 · D-267 验收⑤⑦ + R-183 验收① + R-182 验收④⑥ 端到端收口 · worktree 内无人值守闭环、tracker 合并回归、conventions 新口径与旧口径清理、四条条目回写

- 文件面: `crates/kanzei/tests/unattended_worktree_loop.rs`, `crates/kanzei/tests/tracker_merge_regression.rs`, `.kanzei/project/conventions.md`, `.kanzei/project/goals.md`, `.kanzei/project/architecture/README.md`, `docs/design/deep_parallel_dev.md`, `docs/design/parallel_read_serial_write_orchestration.md`, `docs/design/app_icon.md`, `crates/kanzei-app/src/settings.rs`
- 改动: 1) **端到端集成测试**(D-267 验收⑤ 与 R-183 验收① 是同一条轨迹,两份原方案里只有一份真做了):真 git 仓 + `git worktree add` + mock SSE(照抄 always_allow_bash.rs 的现成形态),主根 kanzei.toml 只放**可手写的 form-C 规则**,以 `KANZEI_PROJECT_ROOT=<主根>` + `KANZEI_NONINTERACTIVE=rules-only` + `KANZEI_ALLOWLIST=<清单>` + `Stdio::null()` 跑 `kz run`,脚本化模型返回:edit 改文件 → bash 跑验证命令 → `git add <具体文件>` → `git commit -m …`。闭环里必须真的走 `git add <file>` 与 `git commit`,以证明 F2 的否决表没有把提交动作误伤。
  **验证命令用一条可控的假构建命令(如 `echo ok`)而不是真 `cargo test`**,并在测试注释与条目进展里如实标注:本机 target 已 53GB、真跑是分钟级且与外层 cargo 抢构建锁;验证的是**授权链路**,不是构建本身。按 conventions §1.25「范围限定词不得缩小」,这一处**必须作为显式待拍板项写进 D-267 进展**,不能默认判成「验收⑤ 已满足」。
2) 实测③固化成回归:三条分支各改 defects.md 同一文件的不同段落,顺序 `git merge --no-ff` 三次,断言全干净且三条进展行一条不少(R-182 验收④)。
3) **conventions 新增一节**「分支干、合并、冲突检测解决、文档一份唯一」,取代「并行查、串行写」旧口径(R-182 内容⑤)。
4) **旧口径残留清理**(证伪三 concerns 第 2 条:两份原方案的验收都会自己判红,因为 files 里没带上残留文件):实测残留在 6 处 —— `.kanzei/project/goals.md`、`.kanzei/project/architecture/README.md`、`docs/design/deep_parallel_dev.md`(两处)、`docs/design/parallel_read_serial_write_orchestration.md`(三处)、`docs/design/app_icon.md`,逐处加「已撤销(2026-08-11)/旧口径」限定或改写。
5) **R-183 内容④ 的正确落点**(两份原方案都错):基础规则模板要写进**新建配置的注释模板**,实测在 `crates/kanzei-app/src/settings.rs:578-586`(今天只有 3 行注释),不是 `docs/templates/*.toml`。模板内容按 D-267 实测清单归纳(`echo`/`head`/`tail`/`awk`/`grep`/`ls`/`Select-Object` + git 只读子集),**每条写具体主根 workdir 而不是 `*`**,并在注释里带上 F8 那句 ACE 警示。
6) 逐条对照 D-267 / R-177 / R-182 / R-183 的验收原文回写四条条目的进展与批次,每项给精确代码位置或可复核证据(conventions §1.25,不得笼统说「已实现」)。
- 测试:
  - `worktree里靠可手写规则完成一次改代码→验证→git提交闭环`(D-267 验收⑤ / R-183 验收① 同一条轨迹)
  - `闭环里的git_add与git_commit没有被否决表误伤`(定向反证:断言这两段的 PermissionResolved 是 allow 而不是 denied)
  - `三条线各改自己那段tracker后顺序合并全干净零丢失`(R-182 验收④)
  - `配置模板可被解析且每条规则合法`:读 settings.rs 的模板文本 → `toml::from_str` 成功 → 每条 form-C 规则过 `argv_rule_is_valid`(防模板与 schema 漂移)
  - `模板不示范无限workdir`:断言模板文本不含 `workdir = "*"`
  - `conventions新口径进了系统提示词`:照 profiles.rs 的现成范式扩一条,断言新一节的关键 token 出现在提示词里
- 验收: ① 四条端到端测试全绿且命名可与验收原文一一对上;② **旧口径清理可机械判定**:`grep -rn "并行查" .kanzei docs crates --include=*.md --include=*.rs | grep -v "已撤销\|旧口径\|-archive\|quarantine" | wc -l` == 0(排除式计数;「全部带限定」不是一条 grep 能判的);③ `grep -n 'workdir = "\*"' crates/kanzei-app/src/settings.rs` 零命中;④ **条目回写可机械判定**:一条检查步断言 R-177/R-182/R-183/D-267 的进展段里每个验收圈号后 200 字内含 `.rs:` 或 `fn ` 形态的证据锚点;⑤ D-267 与 R-177 复杂度均为「大」,收口前跑 `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` 三条全绿(conventions §1.4);⑥ `scripts/verify.ps1` 八步全绿并产出 dist/verification.json。

## 派发波次(5 波可任务级并行)

### 波0(独占)

- 批次: F0
- 不重叠依据: **不可与任何批次同批派发**。给 `Rule` 加两个字段会打断 workspace 全部下游的 27 处结构体字面量,任何同时在跑的线都编不过——文件面不重叠也没用,这正是 R-185 说的「按验收推而不是按根因推」:F0 的验收是 `cargo build --workspace` 通过,而那条验收覆盖整个工作区。F0 本身纯机械、零行为变化,单独跑很快。

### 波1(4 条线并行)

- 批次: F1, F2, F3, F4
- 不重叠依据: 文件面按**验收**核对后完全不重叠:F1=[kanzei-harness/src/permission.rs, kanzei-core/src/runner/drive.rs];F2=[kanzei-harness/src/cmdline.rs(新建), kanzei-harness/src/lib.rs(一行 mod)];F3=[kanzei-harness/src/config.rs, kanzei/src/main.rs, kanzei/tests/worktree_main_root.rs(新建)];F4=[kanzei-app/src/processes.rs, kanzei-app/src/worktree_tests.rs(新建), kanzei-core/src/store/schema.rs]。四条都只**读**对方的类型、不改对方的签名,零语义耦合:F2 是纯新增模块无人调用;F1 只改 bash 分支的匹配顺序,不碰 Rule 形状;F3 只新增取根入口,`discover_project_root` 一个字符不动;F4 只在 kanzei-app 内定字段口径。**已知的共享面只有构建目录**:四条的验收都要跑 cargo,同一个 CARGO_TARGET_DIR 会互相等锁——按既有纪律「构建互踩要等不要修」。

### 波2(3 条线并行)

- 批次: F5, F6, F7
- 不重叠依据: F5=[kanzei-harness/src/permission.rs](仅此一个文件);F6=[kanzei-app/src/run.rs, kanzei-tools/src/{bash,frontend,files}.rs, kanzei-app/src/process_tests.rs];F7=[kanzei/src/main.rs, kanzei-tools/src/{webfetch,websearch}.rs, kanzei/tests/worktree_main_root.rs]。三条互不重叠。语义耦合已逐条证伪:F5 改的是「规则如何匹配资源」,F6 改的是「资源里的 workdir 取自哪棵树」,两者在 `resources_with_ctx` 处交汇但**函数签名不变**,F5 的测试自造资源字符串、F6 的测试自造 handle,互不依赖对方的行为;F7 只改身份键与两个工具的配置取根,与 F5/F6 无共享符号。**F6 与 F7 都动 kanzei-tools 但文件不同**;F7 与 F3 都动 kanzei/tests/worktree_main_root.rs,但 F3 在波1 已合入,F7 是追加而非改写。

### 波3(2 条线并行)

- 批次: F8, F9
- 不重叠依据: F8=[kanzei-harness/src/config.rs](仅此一个文件,全部是告警文案与 schema 键);F9=[kanzei-tools/src/{git,lib}.rs, kanzei-app/src/{processes,main}.rs, kanzei-app/ui/09-sessions.js, scripts/ui-runtime-smoke.mjs]。零重叠。F8 新增的 `non_interactive` 键要到波4 的 F10 才有消费者,本波内是死键(有 unknown_keys 与 merge 测试守着,不是无测试的悬空)。F9 与 F4 都动 processes.rs,但 F4 在波1 已合入。

### 波4(2 条线并行)

- 批次: F10, F11
- 不重叠依据: F10=[kanzei-core/src/runner/{mod,drive}.rs, kanzei/src/main.rs];F11=[kanzei-harness/src/permission.rs, kanzei-app/src/{state,run,process_tests}.rs, kanzei-core/src/store/{processes,schema}.rs]。两条都动 kanzei-core 但**分属 runner/ 与 store/ 两个子模块、文件不重叠**。语义面:F10 改 ask 的**时机**(RunnerConfig.ask_policy),F11 改**规则集内容**(线的六条 tracker deny)——两者在 `Ruleset::evaluate` 汇合,但 F10 完全不读规则、F11 完全不读策略,各自的测试也互不依赖。F11 与 F6 都动 run.rs,F6 在波2 已合入;F11 与 F4 都动 schema.rs,F4 在波1 已合入。

### 波5(2 条线并行)

- 批次: F12, F13
- 不重叠依据: F12=[kanzei/src/main.rs, kanzei-harness/src/permission.rs, kanzei-core/src/runner/{event,drive}.rs, kanzei-app/src/{run,state}.rs, kanzei/tests/noninteractive_run.rs];F13=[kanzei-app/src/{worktree_tests,process_tests}.rs]。F13 是**纯测试批、零生产代码改动**,与 F12 的文件面无交集(F12 不碰 worktree_tests.rs;两者都可能碰 process_tests.rs——**这一处必须排开**:把 F12 里对桌面端事件消费方的断言放进 run.rs 的内联测试,不进 process_tests.rs,F13 独占 process_tests.rs)。

### 波6(独占)

- 批次: F14
- 不重叠依据: **不可并行**。它的验收要跑 `cargo test --workspace` + `cargo clippy --workspace` + `scripts/verify.ps1`,覆盖全部文件面;它还要回写四条条目的进展(tracker 是主根唯一一份,并发写会撞)。而且它是唯一一条端到端验证前面 13 批**合起来**是否成立的批次,必须在其余全部合入之后跑。

## 9 条致命问题的处置

### 1. 【证伪一 FATAL-1】兼容垫片放在 gate 之后是死代码:两方案停止 drive.rs 对 bash 的 normalize 后 value 变成合法 JSON,而配置里 7 条 mangled pattern(含 `/"`)不是合法 JSON,permission.rs:209-212 的 `if value_is_structured && !pattern_is_structured { return false }` 会先行返回,两方案各自修改的兜底比较永远执行不到;两方案还都把这段标成禁区,等于把红线区的改动推给实施者临场发挥。

**改方案,证伪成立且实测复核确认。** 已在仓内核对:第 89 行 pattern `{"command":"git add .kanzei/project; git commit -m /"整理…/"",…}` 的 `/"` 会在 JSON 里提前闭合字符串,解析必失败;而去 mangling 后的 value 解析成功。处置在 F1:**把垫片挪到 gate 之前,并且只允许逐字节相等** —— `if vs && pattern == normalize_resource(value) { return true; }`,不走 wildcard。三点保证:①准入集是 `{V : P 逐字节等于 normalize_resource(V)}`,normalize_resource 确定性,每个 V 只对应唯一一个 P,没有等价类扩张;②`git status` / `git *` 这类真 legacy 纯字符串 pattern 永远不等于结构化 value 的规范化形态,`:494` 的性质由**这一行独立保住**,而不再只依赖它下面的 gate;③gate 本身一个字符不改,保留在垫片之后。同时**否决方案A 的「两侧都 normalize」**(证伪一 concerns 第 3 条也点了):那是把准入集从 `{V: N(V)==P}` 放宽成 `{V: N(V)==N(P)}`,对未规范化的 pattern(如配置第 39 行 `./scripts/release.ps1`)是真实放宽,方向错误。另补两份方案都缺的守卫(证伪一 missing 第 1 条):F1 新增 `gate守卫_结构化value不被非结构化pattern授权`,**直接单测 `resource_match_for_action` 本身**,四种非结构化 pattern × 结构化 value 全断言 false。垫片同时覆盖两类历史规则:7 条非合法 JSON 的,以及 5 条合法 JSON 但被整串小写过的(如第 82 行,证伪一没点到但同样会失配)。

### 2. 【证伪一 FATAL-2】方案A 的 A4 让「总是允许」把整串 command 写进 form-C 的 command 字段,而 A2 的段级匹配会把它切段再要求整段字面相等——多段 pattern 与任何单段都不相等,永不命中;含 `>`/`$` 的更是先判 Undecidable。用户配置里现存 12 条**全部**含 `;`/`|`/`$env:`/`2>&1`/转义引号,即 A4 之后每一次「总是允许」都产出一条本轮生效、重启失效的死规则。

**改方案:整批丢弃 A4,「总是允许」写什么本轮一个字不改。** 证伪成立,且仓内复核追加了一条它没写的加重情节:`generalize_resource` 已确认是恒等函数(config.rs:834-837),`drive.rs:836` 的 `session_rules` 存的正是那条整串 JSON —— 所以本轮内一定放行、重启后一定失配,失败形态是「随机性」而不是「报错」,用户无从归因。采方案B 的取舍:`append_allow_rule` 的 toml_edit 文本级追加路径一个字符不改,`persist_always_allow` 的「写盘成功才返回 AlwaysAllow」控制流不动,CLI main.rs 与桌面 permission_tests.rs 的四条 fail-closed 成对断言保持绿,`always_allow_bash.rs` 的端到端断言也不改。**代价如实写下**:用户点一次 always 仍只攒出一条不可复用的精确规则,配置继续以每命令一条膨胀(D-267 影响①),任务级并行会让膨胀速度乘以线数。收敛路径改由 F8 的启动告警承担(D-267 验收⑥ 的原文就是「消失**或**给出可执行的收敛路径」,二选一)。真正的自动收敛会同时波及 AskReply 枚举、CLI、桌面端三处,留给后续条目。

### 3. 【证伪一 FATAL-3】逃逸 token 表大小写敏感(方案A 同时列 `-c` 与 `-Command` 即自证),叠加 A3 主动教用户收敛成 `workdir="*"`,把「workdir 是授权身份的一部分」架空:`git -C ../../other-repo reset --hard` 单段、无 veto 构造、首 token 命中 `git *`、`-C` 不在表内 → Allow,命令实际在另一个仓库执行。等价的还有 `git --git-dir/--work-tree`、`cargo --manifest-path`;方案B 大小写不敏感能拦 `-C`,但那三个长 flag 同样漏。

**改方案,证伪完全成立。** 两处修正,都在 F2/F5:①**新增 RELOCATION_FLAGS 否决表**(`-C` `--directory` `--chdir` `--cd` `--workdir` `--work-tree` `--git-dir` `--manifest-path` `--config` `--target-dir` `--project` `--prefix` `--root`),**大小写不敏感**,并同时匹配 `--flag value` 与 `--flag=value` 两种形态。判据是可陈述的:凡让命令在规则写的 workdir **之外**取工作目录或配置的 flag,一律 Undecidable → Ask。②**推荐语与模板不再示范 `workdir="*"`**:F8 的收敛文案与 F14 的配置模板都写具体主根路径,`"*"` 保留为用户可显式选择的写法但我们不推荐;F12 的 per-run allowlist 更进一步——workdir 由 CLI 钉成本次运行的 cwd,条目只写 command。补上两份方案都没有的**语义级测试**(证伪一 missing 第 2 条):F5 的 `workdir是授权身份的语义级反证`,把上述五条命令 + 三种大小写变体全部钉死为 Ask 且 reason 点名 flag。

### 4. 【证伪二 FATAL-1】段级闸门是纯 shell 语法过滤器,对「被允许的程序本身是什么」一无所知。两份方案的旗舰规则 `command="cargo *" workdir="*"` 而 cargo 本身就是任意代码执行引擎:`cargo test`/`run`/`build` 按设计编译并运行工作树里的代码与 build.rs;agent 用被允许的 edit 工具写测试/build.rs 源码,被执行的 Rust 就是它选定的。

**判定成立,处置是「接受 + 不假装 + 收窄」,并把它提成需要用户拍板的一条。** 仓内已确认 `crates/kanzei-app/build.rs` 存在、两个可运行 bin 存在,场景可原样复现。三点:①**不假装它是安全边界**——F2 的模块头注释与 F8 的启动告警都要写死契约:「段级闸门只保证被调起的**程序**与**工作目录**是操作员写下的那一组,**不保证被调起的程序自身不能执行任意代码**」。F8 新增 ACE 类程序告警(cargo/node/npm/python/pwsh/awk/… 二十余个),命中即逐条点名「该规则等于在 `<workdir>` 授予任意代码执行」,`workdir="*"` 时措辞升级。②**承认这一条无法用任何黑名单关掉**,理由要说清:危险性在程序语义里而不在 shell 语法里;cargo 子命令白名单也不解决(`cargo test` 本身就编译并运行 build.rs 与测试二进制)。③**收窄可收窄的**:通过 FATAL-3 的重定位表 + workdir 由运行时钉成本次 cwd,把爆炸半径限制在这棵树内;per-run allowlist 是操作员为本次运行显式提供的只读文件,不落盘、不进设置页、不持久化。**残余风险的诚实陈述**:一个能改源码、又被授权跑构建的无人值守 agent,本质上就是在该树内的任意代码执行引擎——这与「人自己在 agent 改完代码后敲 `cargo test`」的风险完全相同,是无人值守编码 agent 的固有属性,不是本机制的缺陷。**这一条必须由用户拍板接受**,写进 D-267 与 R-183 的进展作为显式限定,不能默认判成「已交付安全中间档」。

### 5. 【证伪二 FATAL-2】`cargo build --config <攻击者写的 toml>`(rustc-wrapper 注入)与 `cargo run --manifest-path <外部 crate>`(build.rs 构建期执行)是三个干净 token、无任何被否决字符,连「假设存在子命令白名单」都躲得过;`--config`/`--manifest-path` 不在两份方案的任何否决表里。

**改方案,证伪成立且这条比 FATAL-1 更该修**——它不需要 agent 写任何 build.rs,且突破的是「workdir 是授权身份」这条真红线(注入的配置/清单指向树外)。处置与 FATAL-3 合并:F2 的 RELOCATION_FLAGS 表包含 `--config` `--manifest-path` `--target-dir` `--project` `--root` `--prefix`,F5 有 `cargo build --config C:/x.toml`、`cargo --manifest-path ../o/Cargo.toml test`、`cargo build --target-dir ../o` 三条定向反证。**残余缺口如实标注**:表是黑名单,对 shell 与工具生态不可穷尽(`env X=Y cmd`、`nohup`、`start`、`cmd /c`、`python -m`、被白名单的 pwsh cmdlet 里的 `[System.Diagnostics.Process]::Start(...)` 都不在表内)。缓解是 F2 的**表内容快照测试** + 写进注释的纪律「给表**减项**必须同批新增一条该项对应的反例测试」——这是证伪一 missing 第 4 条要的机械约束,取代两份方案里只写在模块注释里的一句无约束力的话。但必须说清:快照测试守的是「表不被静默缩小」,不是「表是完备的」。完备性在这个方案里不成立,见 FATAL-1 的处置。

### 6. 【证伪三 FATAL-1】R-183 验收③「从 worktree 运行时主根的 permission 规则能命中,有测试直接断言同一条规则在主根与 worktree 下匹配结果一致」在两份方案里没有任何批次、任何测试。实测:.kanzei/kanzei.toml 的 12 条 bash 规则 workdir 硬编码主根,而资源由 `ctx.cwd.join(workdir)` 生成,cwd 一指向 worktree 就一条都匹配不上——D-267 的复现场景原样复发,而两份方案的验收全绿。

**改方案,证伪完全成立,已在 bash.rs:88-99 逐行核对。** 三件事合起来处置,并把验收③ 重述成可判定形态:①**否决字面实现**(把 resource 里的 workdir 值改写成主根):资源会**说谎**——bash 实际仍在 `ctx.cwd` 执行,权限对话框、轨迹、落盘规则都会记一个从未使用的目录,而且等于让 12 条主根规则一夜覆盖所有 worktree,正是「旧规则被默认提权」。两份方案在这一点上判断正确,但都没给出替代的可判定验收。②**可满足的半边(F12)**:per-run allowlist 的 `workdir` **由 CLI 在装配时钉成本次运行的 cwd**,条目只写 `command` —— 于是同一份 allowlist 从主根跑与从 worktree 跑都能命中。验收③ 重述为:「一条测试,同一份 allowlist 分别以 cwd=主根 与 cwd=worktree 装配,断言两次都命中对应工作目录的资源」——这才是「同一条规则在两处匹配结果一致」的可判定形态。方案A 的 `workdir="*"` 也能过这条,但同时废掉 workdir 身份,所以取方案B 的注入写法并补 opt-out 通道。③**可见性半边(F8)**:新增 `bash_rules_pinned_elsewhere(cwd)`,当存在 workdir 钉在 cwd 之外的 bash 规则时启动即告警并点名「这些规则本次一条都不会命中」+ 给出改写成 command/workdir 形态的收敛写法。**明确写下的取舍**:那 12 条旧规则在 worktree 下**不命中是有意的**——它们授权的是主根,不是别处;要在别处生效必须由用户显式改写。这一点要写进 R-183 进展,因为它把验收③ 的字面读法改了,需用户拍板。

### 7. 【证伪三 FATAL-2】方案B 的 B10 给 session_id 加 worktree 后缀,但 `process_session_id(root, process_id)` 的签名推不出 worktree 名,而它有 12 个调用点(conversation.rs 5、run.rs 4、processes.rs 3、state.rs 1);改签名要同批改 9 个不在改动面里的调用点,编进 process.id 则撞上 `id.split('|').next()?.strip_prefix('p')?.parse::<u32>()`,parse 失败会让 filter_map 丢弃该条、`max()` 回退、下次 process_create 生成一个已存在的 p2。

**改方案:本轮明确不做 R-177 内容④,采方案A 的立场并把它提成待拍板项。** 证伪成立,12 个调用点与 processes.rs:159-167 的解析形态已逐行核对。理由三条:①F4 定死 `project_dir` 恒主根后,`#p{n}` 已经保证唯一,后缀带不来唯一性收益;②给既有线的会话改一次名就是让历史集体失联(D-176 红线:身份串变了历史就失联);③收益不抵 12 个调用点的改动面与 id 解析器的隐性契约。**替代交付**:分支名在 `ProcessInfo` 里回显供前端显示(F11 顺带做),用户能看到线跑在哪个分支,只是会话 id 里不体现。**这是需求内容项的零交付,必须由用户拍板**:要么接受把内容④ 降级为「分支名在 ProcessInfo 回显」,要么单开一条条目专门做 session_id 身份串的迁移(那需要一次历史会话改名的迁移方案)。R-177 验收⑥ 的前半(「线 session_id 与主树进程互不覆盖」)由 `#p{n}` 已满足并有测试;后半(「删树后会话历史仍可回放」)由 F13 覆盖。

### 8. 【证伪三 FATAL-3】方案A 的 D2 让 cwd=worktree,但 `files` 工具的 `scan(&ctx.project_root)`/`load_annotations(&ctx.project_root)` 与 `frontend` 工具的 `ctx.project_root.join(rel)` 仍读主树,而 read/glob/grep/write/edit 已走 worktree——同一个 agent 在两棵树之间读写:frontend 读主树内容 → 照它构造 edit 的 old_string → edit 按 cwd 落 worktree,分支改过该文件时匹配失败反复重试,没改过时基于陈旧上下文改对了地方(R-185 点名的语义撞车)。方案A 把这一条放进 tradeoffs 说「应单开缺陷」,但没有任何批次要求登记。

**改方案:证伪成立(已在 frontend.rs:181/242、files.rs:382/386 逐行核对),这三处收进 F6 与 cwd 分离同批交付。** 这正是任务卡里点名的「按验收推而不是按根因推」的样本:R-177 验收② 要求「线内写代码落在 worktree」,而只要 frontend/files 还读主树,这条验收就是假绿——所以它们属于 F6 的验收面,不是可以推给后续条目的旁支。同批还带上 `background_workdir_breach`(不改则线里所有 background bash 被无条件拒,是 F6 自己引入的回归)。判据统一写进注释:**凡 `.kanzei/**` 资产走 project_root,凡仓库源码走 cwd**。`ManagedSnapshot::capture` 保持取主根不变(托管文档只有主根一份是对的),但补一条测试固化这个事实免得后来人以为是漏了。`git_batches::commit_subjects` 本轮**只定性加注释不改行为**——勘察未能在本轮确认它的实际取根形态与消费链影响,不做没读透的改动,单开缺陷登记(见 not_doing)。

### 9. 【证伪三 FATAL-4】D-267 验收⑤ 与 R-183 验收① 是同一条轨迹(worktree 里 stdin 关闭时靠可手写规则完成「改代码 → cargo test → 提交」),方案A 没有任何批次交付它:C3 只让 mock 模型发一条 `cargo --version`(单命令、无编辑、无提交、不在 worktree 里),D5 的端到端是测试代码自己写文件+git commit,完全不经过 kz run 的授权链路。

**改方案:采方案B 的 B13 并加固,落成 F14。** 证伪成立。F14 的闭环必须真的经过 kz run 的授权链路,且脚本化模型的动作序列包含 `git add <具体文件>` 与 `git commit -m …` —— 这同时是 F2 否决表的**回归守卫**:证伪一 concerns 第 6 条指出方案B 的 `.` 与 `-e` 无条件否决会拦掉 `git add .` 和 `grep -e`,F2 已按此改成「首 token 才算 `.`/`source`」「`-e` 只在段首是解释器时才算」,F14 有 `闭环里的git_add与git_commit没有被否决表误伤` 定向反证钉住。**验证命令用可控假构建命令而不是真 `cargo test`**,理由与方案B 相同(本机 target 53GB、分钟级、与外层 cargo 抢构建锁),但处置更严:按 conventions §1.25「范围限定词不得缩小」,这一处**作为显式待拍板项写进 D-267 进展**,不只是写在 tradeoffs 里——否则收活时会变成「验收⑤ 已满足」的默认判定。

## 本轮明确不做

- **R-182 内容①(撤销不变量 3 与 `ExecutionPolicy::ReadParallelWriteSerial` 的全程串行强制)** —— 不在本轮第一梯队四条内。相关断言在 `crates/kanzei-core/src/runner/tool_exec.rs:367`、`crates/kanzei-harness/src/orchestration.rs:466`、`crates/kanzei/tests/parallel_scouting_under_serial_writer.rs:269`。撤锁必须与 R-184(协作可见性)同批,否则就是拆了护栏不告诉司机——R-182 自己的正文与 parallel_lines_ui.md §9 都这么写。**后果要说清:本轮交付后 R-182 不能标交付**,条目里须显式记「内容①/验收③ 留待 R-184 同批」。本轮只做内容②(主根重定向)与内容④⑤。
- **R-177 内容④(session_id 加 worktree 后缀)** —— 见 fatal_resolutions 第 7 条。12 个调用点 + id 解析器的隐性契约 + D-176 的一次性改名,收益不抵风险。替代:分支名在 ProcessInfo 回显。**需用户拍板降级或单开条目。**
- **「总是允许」改用新形态落盘** —— 见 fatal_resolutions 第 2 条。整批丢弃,配置继续累积旧形态,收敛路径由启动告警承担。
- **kanzei.toml 的自动迁移**(把 12 条旧结构化规则重写成 form C)—— 改用户手写的配置文件风险高于收益;D-267 验收⑥ 明写「消失**或**给出可执行的收敛路径」,取后者。
- **启动告警探针的 workdir 保真**(`bash_permission_warnings` 今天探针写死 `"."`)—— 改签名要同批动 `crates/kanzei/src/main.rs:128-130` 与 `crates/kanzei-app/src/run.rs:1241` 两个打印点,而这两个文件在本轮已分别被 F3/F7 与 F6/F11 占用,同批会造成两波之间的文件冲突。告警是建议性的,失真方向是偏保守。**单开缺陷登记**,并在 F8 的注释里点名该缺口。
- **`normalized_project_root` 拆成 `normalize_root` / `discover_and_normalize`** —— 方案B 的 B12 提出用编译器逐个报,但实测该符号 42 处、分布在 processes.rs 10 / run.rs 8 / conversation.rs 6 / docs.rs 4 / projects.rs 3 / mobile.rs 2 / state.rs 1 / main.rs 1,拆函数会一次性打断 6 个不在任何批次 files 里的文件,与 conventions §2 冲突。**单开条目**,做成独立一轮。
- **二十余处取根漏项** —— 实测清点:`subagents.rs` 的 quick_req/defect_review 两个桌面端 tracker 写入口(load(&cwd) + discover,且构造的 ToolCtx 不带 run_id/两把键);`docs.rs` 的 6 处 discover + 3 处 normalized_project_root + :343 的 `ToolCtx::discovering`;`memory.rs` 的另外 6 处 discover;`run.rs:1253/1387/1452`;`files_view.rs:18/261`;`projects.rs` 5 处;`settings.rs:543/602`;以及 4 处 `KanzeiConfig::load(Path::new("."))`(update.rs 两处、fast_model.rs、settings.rs)按 kzapp 进程 cwd 取配置、与当前项目完全脱钩。这些今天全靠「前端恒传 currentProject」这条**约定**成立。**归成一条缺陷登记**(它们是同一个病),本轮不改。相应地,F3 的验收 grep 只声称覆盖 `crates/kanzei/src`,不声称「全仓发现式取根已清零」——两份原方案的验收都只扫 CLI 却给出全仓清零的假信号。
- **`git_batches::commit_subjects` 的取根改判** —— 本轮只在 F6 里加注释定性(按同一判据应取 cwd 的 git),不改行为。勘察未能确认它的实际形态与消费链影响,不做没读透的改动;**单开缺陷登记**。
- **`run_metrics` 的裸 `PathBuf::from(&project_dir)`** 与 **`Limits::barrier_timeout_secs` 的两处漏接线** —— 都与本轮四条无关(conventions §2)。但 F8 新增 `non_interactive` 键时必须照同一模板把「struct + unknown_keys + merge overlay + 告警」四处一次做齐,避免复制同一个 bug;这两条**单开缺陷**。
- **语义撞车检测**(A 把某签名重构成形态①、B 按形态②写,git 一字不报)—— R-182 边界已明确列为已知缺口,R-185 是它的事前对策条目。本轮不造机制。
- **桌面端的无人值守** —— R-183 边界明写不做(桌面端有 UI 可问)。`AskPolicy` 挂在 `RunnerConfig` 且 `Default = Interactive`,桌面端不受影响由类型层面保证,有 `grep -rn 'AskPolicy' crates/kanzei-app` 零命中的机械验收。
- **worktree 内 `.kanzei` 副本的物理排除**(sparse-checkout / skip-worktree)—— R-177 边界明写是可选纵深防御,不在本条、不阻塞交付。

## 风险与需拍板

- **最重的一条(需用户拍板)**:段级闸门无法让 `cargo`/`node`/`pwsh` 这类程序在无人值守下「安全」运行——它们按设计执行工作树里的代码,而 agent 有 edit/write。本轮的处置是不假装(明写契约边界 + ACE 告警 + 重定位 flag 否决 + workdir 收窄到本次 cwd),不是消除。**如果用户要的是「即使模型被劫持也不能执行任意代码」,本轮这套东西达不到,必须降级为:无人值守只允许真正的叶子命令(ls/echo/git status/head/tail/grep/Select-Object),构建与测试仍需人在场。** 这个降级方案是自洽的、可立即交付的(F2/F5/F8/F12 全部不变,只是 F14 的闭环里不含构建步骤,D-267 验收⑤ 相应重述),但它交付不了「三条线各自跑到底」的原始诉求。请拍板走哪一条。
- **D-267 验收⑤ 的字面缺口**:F14 用可控假构建命令代替真 `cargo test`(本机 target 53GB、分钟级、与外层 cargo 抢锁),验证的是**授权链路**不是构建本身。按 conventions §1.25 必须作为显式限定写进进展并由用户接受,不得默认判成已满足。
- **R-183 验收③ 被重述**:字面读法(worktree 内按主根解析 workdir)会让资源说谎,本轮改为「per-run allowlist 的 workdir 由运行时注入 + 启动告警点名旧规则失配」。副作用是**用户现有的 12 条规则在 worktree 下一条都不命中**——这是有意的(它们授权的是主根),但用户第一次从 worktree 跑 `kz` 时会看到一屏告警。需要接受这个口径。
- **F5 的 form-C deny 会改变 yolo 语义**:一旦配置里出现任何一条 form-C deny,所有不可静态判定的命令(含重定向、`$(...)`、后台 `&`)都会回到 Ask,即使存在 `resource="*"` 的整体放行。这是关掉「用 form-C deny 收紧 yolo 反而给假安全感」这个洞的代价,是用户显式写下 deny 换来的提严,但要在告警里说清,否则会被当成回归。
- **否决表偏严的可用性代价**:D-267 实测清单里的 ①(`echo "EXIT=$?"`)本轮**放行**(裸 `$?` 不算命令替换),④(`cargo test 2>&1 | Select-Object -Last 40`)本轮**仍拦**(`2>&1` 命中重定向)。两份原方案在裸 `$` 上口径还不一致。这意味着模板即便收敛也覆盖不了实测里最高频的重定向那一类,agent 需要学会拆命令。**裸 `$VAR`/`$?` 到底放不放行需要用户拍板**——本轮取「放行」是因为实测清单①明确点名拆掉尾部 echo 后同一条命令直接放行,说明外部参照实现也是这个口径。
- **F0 独占一波会拖慢整体节奏**:给 Rule 加字段是全仓编译面改动,无法与任何批次并行。它本身很快(纯机械 27 处),但它是整条 D-267 线的前置,不能省。
- **波1~波5 的五组并行都共享 CARGO_TARGET_DIR**:各线的验收都要跑 cargo,同一个 target 目录会互相等构建锁,并行的真实加速比会低于线数。按既有纪律「构建互踩要等不要修」,不要为此给各线单独开 target(本机盘剩余空间放不下,D-268 已记)。
- **F12 与 F13 都可能碰 process_tests.rs**,已在派发说明里排开(F12 的桌面端断言放 run.rs 内联测试)。派发时必须把这条约束原文交给两条线,否则它们各自都会「顺手」写进 process_tests.rs。
- **F6 与 F5 在 `resources_with_ctx` 处语义交汇**:F6 改资源里的 workdir 取自哪棵树,F5 改规则如何匹配资源。两者签名不变、测试各自造数据,判定为可并行;但合并后第一次同时生效时,「worktree 下旧规则全失配」这个现象会第一次真实出现——F8 的告警要在那之前落地,否则用户会在没有任何提示的情况下撞上它。派发顺序上 F8 在波3、F6 在波2,**中间有一波的窗口期**,需要在 F6 的提交说明里点名这个已知窗口。
- **R-182 与 R-177 本轮都不能标交付**:R-182 缺内容①/验收③(留 R-184 同批),R-177 缺内容④(需拍板)。条目状态要如实停在 [doing] 并逐条列出未覆盖项,不能因为大部分批次绿了就翻 [fixed]。

