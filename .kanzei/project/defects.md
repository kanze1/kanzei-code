# Defects

## D-152 单元测试在开发机上执行伪造安装器,弹出「与 64 位 Windows 不兼容」 [fixed] (high)
- 原标题: Windows 安装包在 64 位 Windows 上提示不兼容(误判,见下)
- 复现: 跑 `cargo test -p kanzei-app` 时 Windows 弹出"由于与64位版本的Windows不兼容，此程序或功能无法运行"，路径 `AppData\Local\Temp\kz-helper-*\kanzei-setup.exe`。
- 标签: 发布
- 误判澄清: **发布产物本身没有问题**。①报错路径里的 `kz-helper-<pid>` 是 `install_helper_waits_for_the_caller_to_exit_before_installing` 这条单测建的临时目录,不是真安装包所在的 `%TEMP%\kanzei-setup.exe`;②实测 dist/kanzei-setup-430d6d6.exe 的 PE 头 machine=0x14c(32 位),这是 NSIS 的常规形态——32 位安装器在 64 位 Windows 上经 WoW64 正常运行,并负责安装 64 位负载,产物名里的 x64 指的是负载架构而非安装器自身;③本会话已用该安装包成功静默安装四次(exit 0,安装后构建号逐次核验一致)。
- 真实根因: 上述单测为了验证"helper 必须等发起方退出",让 `run_install_helper` 跑完整流程,而它写进临时目录的是一个 23 字节的假 exe(`MZ not-a-real-installer`);等待结束后 helper 真的去 `Command::new(installer).arg("/S")` 执行它,Windows 无法把它当作有效映像加载,于是报架构不兼容。**测试不该在开发机上启动伪造可执行文件**。
- 修复: 抽出 `wait_for_parent_exit(pid, timeout)` 纯时序函数,单测只验这条不变量(父进程活着时等满超时、父进程不存在时立即放行),不再触碰执行环节;整条 helper 的执行分支不进单测。副作用:该测试从 30 秒降到约 1 秒。
- 验收: cargo test -p kanzei-app 通过且不再弹出任何 Windows 对话框;发布安装包架构核验记录在案。
- 优先级: P0
- 证据等级: E2
- 不变量: 测试:不得在开发机上产生真实副作用
- refs: D-124
