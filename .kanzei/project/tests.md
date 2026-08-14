# Test Runs

## T-1786705800 R-249 批2 截图通道: 实窗抓取验证 + 全量 [passed]
- 命令: KZ_SHOT_OUT=<png> cargo test -p kanzei-app screenshot_live -- --nocapture; cargo test --workspace; cargo clippy --workspace --all-targets
- 摘要: 实窗验证三轮才对——①未声明 DPI 感知,GetWindowRect 返回虚拟化坐标(2582px 窗口报成 1295px),抓到横跨多窗口的错误区域,looks_blank 放行、用例假绿;②补 DPI 感知后矩形正确,但屏幕 DC 抓取拿到的是压在上面的编辑器界面(完全遮挡),内容丰富仍然假绿;③改 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染后,在窗口被完全遮挡状态下抓到 kzapp 自己的完整界面 2582×1390,人眼比对与用户实拍逐项一致。全量 26 个测试二进制全绿,clippy 零告警
- 关联: R-249
- 收尾: 1786705800

