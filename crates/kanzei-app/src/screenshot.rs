//! R-249 批2:抓取真实运行窗口的像素,让 UI 自查从「读结构数值」变成「看得见」。
//!
//! **为什么不走 CDP**:仓里确实有 WebView2 的 DevTools 协议通道(R-101 E2 harness
//! 的 `KANZEI_E2E_CDP`),`Page.captureScreenshot` 也确实更准。但那条要开远程调试
//! 端口,生产运行常开一个调试口不值当——自查是日常动作,不该以「一直开着调试」
//! 为代价。
//!
//! **为什么不走 html2canvas 一类**:那是拿 DOM 重画一遍,不是窗口真实的渲染结果。
//! 而 UI 自查要查的恰恰是「渲染出来跟我想的不一样」——重画一遍只会把 bug 一起
//! 重画掉,查不出任何东西。
//!
//! 于是走 Win32 的 `PrintWindow` + `PW_RENDERFULLCONTENT`:让窗口把自己**离屏**
//! 画一遍。那个 flag 正是为硬件合成的 Chromium/WebView2 加的,关键性质是
//! **免疫遮挡**——2026-08-14 实测:kzapp 被编辑器完全盖住时仍抓到它自己的完整界面。
//!
//! **为什么不能只用屏幕 DC BitBlt**(第一版就是那么写的,踩了):屏幕抓取拿的是那块
//! 矩形上**可见**的像素。窗口被盖住时抓到的是压在上面那个应用的界面——一张内容
//! 丰富、[`looks_blank`] 完全放行的图。把它交给模型,模型会当成 kanzei 的界面来
//! 描述。自举跑的时候窗口多半在别的窗口后面,所以这不是边角情况。屏幕抓取只在
//! `PrintWindow` 失效**且**本窗口是前台时才作为回退。
//!
//! 剩下的边界:最小化 / 不可见 → 抓取前拒绝;整幅空白 → [`looks_blank`] 拦下;
//! 两条都不成立又不是前台 → 报错,不返回可能属于别人的画面。

/// 抓取失败或结果不可信时的说明。调用方直接把它当错误文本回给模型。
pub(crate) type CaptureError = String;

/// 判定一张 RGBA 位图是不是「等于没抓到」。
///
/// 抓取 API **不会失败**:合成层还没画完时,它会安安静静地给你一张纯黑或纯白的图。
/// 把这种图交给模型是所有结果里最坏的一种:它会以为自己看过界面了,然后开始描述
/// 一片空白,或者更糟——照着 DOM 结构编出「看起来正常」。宁可报错。
///
/// 注意它**管不了遮挡**:被盖住时抓到的是别人的界面,内容丰富,这里一律放行。
/// 那条靠 `PrintWindow` 的离屏渲染从根上避免,不靠事后判图。
///
/// 判据取「不同颜色数」而不是「是否全黑」:纯白、纯灰、以及只有一两种颜色的
/// 合成残留同样是没抓到。真实界面即使极简也远不止 8 种颜色。
pub(crate) fn looks_blank(rgba: &[u8]) -> bool {
    let mut seen: Vec<[u8; 3]> = Vec::with_capacity(9);
    let (pixels, _remainder) = rgba.as_chunks::<4>();
    for pixel in pixels {
        let key = [pixel[0], pixel[1], pixel[2]];
        if !seen.contains(&key) {
            seen.push(key);
            if seen.len() > 8 {
                return false;
            }
        }
    }
    true
}

/// RGBA 缓冲编码成 PNG 字节。
pub(crate) fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, CaptureError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG 头写入失败: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("PNG 数据写入失败: {e}"))?;
    }
    Ok(out)
}

#[cfg(windows)]
pub(crate) fn capture_window(hwnd: isize) -> Result<(Vec<u8>, u32, u32), CaptureError> {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::HDC;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, IsIconic, IsWindowVisible, PW_RENDERFULLCONTENT,
    };
    // windows-sys 0.61 导出了 PW_RENDERFULLCONTENT 常量却没导出 PrintWindow 函数,
    // 自己声明这个 user32 符号。签名照 Win32 文档。
    unsafe extern "system" {
        fn PrintWindow(hwnd: HWND, hdc_blt: HDC, flags: u32) -> i32;
    }

    let hwnd = hwnd as HWND;
    // 守卫先于抓取:最小化时 BitBlt 拿到的是桌面上别的东西,不是「一张坏图」而是
    // 「一张别人的好图」——那比黑图更容易骗过 looks_blank。
    unsafe {
        if IsIconic(hwnd) != 0 {
            return Err("窗口处于最小化状态,抓不到界面。请先还原窗口再重试。".into());
        }
        if IsWindowVisible(hwnd) == 0 {
            return Err("窗口当前不可见,抓不到界面。".into());
        }
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err("取窗口矩形失败".into());
    }
    let width = (rect.right - rect.left).max(0);
    let height = (rect.bottom - rect.top).max(0);
    if width == 0 || height == 0 {
        return Err("窗口尺寸为 0,抓不到界面。".into());
    }

    // 遮挡判定:本窗口是不是前台窗口。只影响回退路径——屏幕抓取只在它是前台时
    // 才可信,否则抓到的是压在上面那个应用的像素。这条不是保守起见,是实测踩出来
    // 的:第一版只有屏幕抓取,验证时 kzapp 被编辑器完全盖住,抓出来是一整张编辑器
    // 界面,内容丰富、looks_blank 放行,差点就把别人的界面当成 kanzei 的交给模型。
    let is_foreground = unsafe { GetForegroundWindow() } == hwnd;

    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return Err("取屏幕 DC 失败".into());
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        let old = SelectObject(mem_dc, bitmap.cast());

        // 首选 PrintWindow + PW_RENDERFULLCONTENT:让窗口把自己**离屏**画一遍,
        // 不受遮挡影响。那个 flag 就是为硬件合成的 Chromium/WebView2 加的,没有它
        // 拿到的多半是空白。但它对 WebView2 并非总能成——所以下面还要判空回退。
        let printed = PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT);
        let mut blit = printed;
        // PrintWindow 不成时,只有在本窗口是前台(没有东西压着)才允许退回屏幕抓取。
        // 不是前台就宁可失败——返回别人的界面比返回错误坏得多。
        if printed == 0 && is_foreground {
            blit = BitBlt(
                mem_dc, 0, 0, width, height, screen_dc, rect.left, rect.top, SRCCOPY,
            );
        }

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // 负高度 = 自上而下的行序。省掉手工翻转,也少一处会写错的地方。
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
        let copied = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            buffer.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        DeleteObject(bitmap.cast());
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if blit == 0 {
            return Err(if is_foreground {
                "窗口画面抓取失败(PrintWindow 与 BitBlt 都返回 0)".into()
            } else {
                "窗口当前被其它窗口遮挡,且离屏抓取(PrintWindow)不可用。请把 kanzei                  窗口切到前台后重试——绝不要按遮挡时抓到的画面下判断。"
                    .to_string()
            });
        }
        if copied == 0 {
            return Err("位图取出失败(GetDIBits 返回 0)".into());
        }

        // GDI 给的是 BGRA,PNG 要 RGBA;顺手把 alpha 拉满——BitBlt 出来的 alpha
        // 通道是未定义的,原样写进 PNG 会得到一张全透明的图。
        for pixel in buffer.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        if looks_blank(&buffer) {
            return Err("抓到的画面是空白的(窗口可能被其它窗口遮挡、或正在重绘)。\
                 把 kanzei 窗口切到前台后重试;不要按这张图描述界面——它没有内容。"
                .into());
        }
        Ok((buffer, width as u32, height as u32))
    }
}

#[cfg(not(windows))]
pub(crate) fn capture_window(_hwnd: isize) -> Result<(Vec<u8>, u32, u32), CaptureError> {
    Err("窗口截图目前只实现了 Windows 平台".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, color: [u8; 4]) -> Vec<u8> {
        color
            .iter()
            .copied()
            .cycle()
            .take(width * height * 4)
            .collect()
    }

    #[test]
    fn blank_detection_catches_solid_black_and_white() {
        // 这两种是 BitBlt 抓空时最常见的产物。
        assert!(looks_blank(&solid(16, 16, [0, 0, 0, 255])));
        assert!(looks_blank(&solid(16, 16, [255, 255, 255, 255])));
    }

    #[test]
    fn blank_detection_tolerates_up_to_eight_colors() {
        // 合成残留可能有两三种色块,同样算没抓到;真实界面远不止 8 色。
        let mut buffer = solid(8, 8, [0, 0, 0, 255]);
        for (index, pixel) in buffer.as_chunks_mut::<4>().0.iter_mut().enumerate().take(5) {
            pixel[0] = index as u8 * 10;
        }
        assert!(looks_blank(&buffer), "6 种颜色仍应判为空白");
    }

    #[test]
    fn real_looking_image_is_not_blank() {
        let mut buffer = solid(32, 32, [0, 0, 0, 255]);
        for (index, pixel) in buffer.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel[0] = (index % 251) as u8;
            pixel[1] = (index % 253) as u8;
        }
        assert!(!looks_blank(&buffer));
    }

    /// 对着**真实运行的 kanzei 窗口**抓一张,校验不是空白并落盘供人眼复核。
    ///
    /// 默认跳过:它依赖一个正在运行且未被遮挡的窗口,进 CI 必红。这不是摆设——
    /// 截图这条路上「编译通过」和「抓得到画面」之间隔着 WebView2 的合成层,
    /// 单元测试再多也证明不了后者。改动 capture_window 后必须手动跑一次:
    ///   KZ_SHOT_OUT=<路径.png> cargo test -p kanzei-app screenshot_live -- --nocapture
    #[cfg(windows)]
    #[test]
    fn screenshot_live_window_is_not_blank() {
        let Ok(out) = std::env::var("KZ_SHOT_OUT") else {
            eprintln!("跳过:未设 KZ_SHOT_OUT(本用例需要真实窗口,不进常规门禁)");
            return;
        };
        // 先声明 per-monitor DPI 感知,再取任何窗口坐标。
        //
        // 这一步不是可选的:缩放不是 100% 时,非 DPI 感知的进程从 GetWindowRect
        // 拿到的是**虚拟化后**的矩形(实测 2000px 宽的窗口报成 1295px),BitBlt 于是
        // 从错误区域取像素——抓出来横跨好几个窗口。那张图内容丰富,looks_blank
        // 放行,用例假绿。生产路径不受此影响(kzapp 是 DPI 感知进程),但正因为
        // 如此,验证进程必须对齐到同一坐标系,否则验的根本不是生产那条路。
        use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
        };

        // 按**进程名**认窗口,不按标题。标题匹配踩过一次坑:本仓项目名就叫
        // kanzei-code,编辑器窗口标题里带这几个字,于是「找 kanzei 窗口」找到的是
        // 编辑器——抓出来是一张漂亮的、完全无关的图,用例照样绿。
        unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> i32 {
            unsafe {
                if IsWindowVisible(hwnd) == 0 {
                    return 1;
                }
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == 0 {
                    return 1;
                }
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if process.is_null() {
                    return 1;
                }
                let mut path = [0u16; 512];
                let mut len = path.len() as u32;
                let ok = QueryFullProcessImageNameW(
                    process,
                    0 as PROCESS_NAME_FORMAT,
                    path.as_mut_ptr(),
                    &mut len,
                );
                CloseHandle(process);
                if ok != 0 {
                    let exe = String::from_utf16_lossy(&path[..len as usize]).to_lowercase();
                    if exe.ends_with("kzapp.exe") {
                        *(param as *mut isize) = hwnd as isize;
                        return 0;
                    }
                }
                1
            }
        }

        let mut found: isize = 0;
        unsafe { EnumWindows(Some(visit), &mut found as *mut isize as LPARAM) };
        assert!(found != 0, "没找到 kzapp.exe 的可见窗口——先把桌面端开起来");

        let (rgba, width, height) = capture_window(found).expect("抓取应成功");
        assert!(
            !looks_blank(&rgba),
            "抓到的是空白画面——窗口被遮挡或合成层没画完"
        );
        let png = encode_png(&rgba, width, height).expect("编码应成功");
        std::fs::write(&out, &png).expect("落盘应成功");
        eprintln!("已抓取 {width}×{height},PNG {} 字节 → {out}", png.len());
    }

    #[test]
    fn png_roundtrip_preserves_size() {
        let buffer = solid(4, 3, [10, 20, 30, 255]);
        let png = encode_png(&buffer, 4, 3).expect("编码应成功");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "不是 PNG 魔数");
        let decoder = png::Decoder::new(png.as_slice());
        let reader = decoder.read_info().expect("应能解回");
        assert_eq!(reader.info().width, 4);
        assert_eq!(reader.info().height, 3);
    }
}
