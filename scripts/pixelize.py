"""kanzei 代言人资产处理:抠白底 + 色阶拉伸 + 降采样 + 限色。

用法:
    python pixelize.py 输入.png 输出.png --preview
    python pixelize.py 输入.png 输出.png --gamma 0.8      # 整体提亮
    python pixelize.py 输入.png 输出.png --keep-bg        # 源图已是透明底

GPT 出的图通常是白底 RGB,没有 alpha。默认走连通域抠底:只有与画布边界
相连的近白区域会被抠掉,角色内部的高光(眼神光等)保住。
"""
import argparse
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

# style.css 的 --memory-flow ramp,由暗到亮。改这里就能换整套配色。
PALETTE = [
    (0x2A, 0x21, 0x14),
    (0x4A, 0x3A, 0x1C),
    (0x6B, 0x53, 0x25),
    (0x8A, 0x63, 0x28),
    (0xA8, 0x7C, 0x30),
    (0xD9, 0xA8, 0x3F),
    (0xFF, 0xE3, 0xA0),
]
LUMA = np.array([0.299, 0.587, 0.114])


def cut_background(rgb, white=235, desat=14):
    """返回 alpha:与边界连通的近白区域记 0,其余 255。"""
    mx, mn = rgb.max(2), rgb.min(2)
    candidate = (mn > white) & ((mx - mn) < desat)
    labels, count = ndimage.label(candidate)
    if not count:
        return np.full(rgb.shape[:2], 255, np.uint8)
    edge = set(labels[0]) | set(labels[-1]) | set(labels[:, 0]) | set(labels[:, -1])
    edge.discard(0)
    background = np.isin(labels, list(edge))
    return np.where(background, 0, 255).astype(np.uint8)


def pixelize(src, dst, size, gamma=1.0, keep_bg=False, alpha_threshold=128, preview=False):
    img = Image.open(src)
    rgb = np.asarray(img.convert("RGB")).astype(np.float64)

    if keep_bg:
        alpha = np.asarray(img.convert("RGBA"))[..., 3]
    else:
        alpha = cut_background(np.asarray(img.convert("RGB")))

    solid = alpha > 0
    if not solid.any():
        raise SystemExit("抠底后没有前景了 —— 源图可能整张接近白色,改用 --keep-bg")

    # 前景自己占满 ramp:GPT 出图常把人物压在暗端,不拉伸的话七档只用到两档。
    lum = rgb @ LUMA
    lo, hi = np.percentile(lum[solid], [2, 98])
    norm = np.clip((lum - lo) / max(1e-6, hi - lo), 0, 1) ** gamma

    # premultiply 再缩,否则透明区把轮廓边缘拉暗一圈。
    pm = np.stack([norm * (alpha / 255.0), alpha / 255.0], -1) * 255.0
    small = np.asarray(
        Image.fromarray(pm.astype(np.uint8)[..., [0, 0, 0, 1]]).resize(size, Image.BOX)
    ).astype(np.float64)

    sa = small[..., 3:4] / 255.0
    sn = np.clip(np.divide(small[..., :1], np.where(sa > 0, sa, 1.0)) / 255.0, 0, 1)

    # 单色 ramp 下按亮度分档比 RGB 最近邻更均匀,不会因色相偏差跳档。
    pal = np.array(PALETTE, np.uint8)
    idx = np.rint(sn[..., 0] * (len(PALETTE) - 1)).astype(int)
    out_rgb = pal[idx]

    out_a = np.where(small[..., 3:4] >= alpha_threshold, 255, 0).astype(np.uint8)
    out_rgb = np.where(out_a > 0, out_rgb, 0)
    out = Image.fromarray(np.concatenate([out_rgb, out_a], -1))
    out.save(dst)

    visible = out_a[..., 0] > 0
    hist = np.bincount(idx[visible], minlength=len(PALETTE))
    used = int((hist > 0).sum())
    share = " ".join(f"{h * 100 // max(1, visible.sum())}%" for h in hist)
    print(f"{src.name} -> {dst.name}  {size[0]}x{size[1]}")
    print(f"  不透明 {int(visible.sum())}/{visible.size} ({visible.mean() * 100:.0f}%)  用色 {used}/{len(PALETTE)}")
    print(f"  色阶分布(暗→亮): {share}")

    if preview:
        p = dst.with_name(dst.stem + "-x4.png")
        out.resize((size[0] * 4, size[1] * 4), Image.NEAREST).save(p)
        print(f"  预览 -> {p.name}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src", type=Path)
    ap.add_argument("dst", type=Path)
    ap.add_argument("--size", default="128x192")
    ap.add_argument("--gamma", type=float, default=1.0, help="<1 提亮,>1 压暗")
    ap.add_argument("--keep-bg", action="store_true", help="源图已是透明底,跳过抠底")
    ap.add_argument("--alpha-threshold", type=int, default=128)
    ap.add_argument("--preview", action="store_true")
    a = ap.parse_args()
    w, h = (int(v) for v in a.size.lower().split("x"))
    pixelize(a.src, a.dst, (w, h), a.gamma, a.keep_bg, a.alpha_threshold, a.preview)


if __name__ == "__main__":
    main()
