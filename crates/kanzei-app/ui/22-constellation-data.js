// UI2-0926 #10 对话背景(星座背景)的纯数据:零 import、零 DOM,node 冒烟可直接 import。
// 设计见 docs/design/ui_chat_backdrop.md。

// kanzei 标志:与 index.html 空态 .logo-mark / 15-views-misc.js EMPTY_STATE_LOGO 同一组路径(viewBox 64)。
// weight:线宽 7 的主笔画 = 2,线宽 3 的电路臂 = 1。role 语义取自 docs/design/app_icon.md「设计概念」:
// 竖笔 = 项目执行主干(trunk),三条平行臂 = 记忆层(memory),右下粗笔 = 行动(action);
// hub 是三层记忆汇入的决策点(下腿起点)。彗星按事件语义沿对应角色的边走,动画因此「有意义」。
export const KANZEI_LOGO_STROKES = {
  viewBox: 64,
  hub: [21, 33],
  strokes: [
    { points: [[14, 8], [14, 56]], weight: 2, role: "trunk" },
    { points: [[21, 33], [44, 56]], weight: 2, role: "action" },
    { points: [[21.5, 31.5], [43, 8]], weight: 1, role: "memory" },
    { points: [[25.5, 35], [50, 8]], weight: 1, role: "memory" },
    { points: [[29.5, 38.5], [57, 8]], weight: 1, role: "memory" },
  ],
};

// 真实星座:[id, 中文名, 赤经(小时, J2000), 赤纬(度, J2000), 视星等]。
// 取 Yale Bright Star Catalogue / Hipparcos 公开值(事实数据),只用于相对位置投影,角分级精度足够;
// 投影后东在左、北在上(与星图一致),冒烟用球面几何核对角距与指极星(scripts/ui-constellation-smoke.mjs ③④)。
export const STAR_PRESETS = {
  "big-dipper": {
    stars: [
      ["dubhe", "天枢", 11.062131, 61.751028, 1.79],
      ["merak", "天璇", 11.030686, 56.382417, 2.37],
      ["phecda", "天玑", 11.897180, 53.694750, 2.44],
      ["megrez", "天权", 12.257100, 57.032611, 3.31],
      ["alioth", "玉衡", 12.900486, 55.959833, 1.77],
      ["mizar", "开阳", 13.398761, 54.925361, 2.23],
      ["alkaid", "摇光", 13.792344, 49.313278, 1.86],
      ["alcor", "辅", 13.420428, 54.987972, 3.99],
    ],
    edges: [["dubhe", "merak"], ["merak", "phecda"], ["phecda", "megrez"], ["megrez", "dubhe"],
      ["megrez", "alioth"], ["alioth", "mizar"], ["mizar", "alkaid"]],
  },
  orion: {
    stars: [
      ["betelgeuse", "参宿四", 5.919531, 7.407056, 0.50],
      ["rigel", "参宿七", 5.242297, -8.201639, 0.13],
      ["bellatrix", "参宿五", 5.418850, 6.349694, 1.64],
      ["mintaka", "参宿三", 5.533444, -0.299083, 2.23],
      ["alnilam", "参宿二", 5.603558, -1.201917, 1.69],
      ["alnitak", "参宿一", 5.679314, -1.942861, 1.77],
      ["saiph", "参宿六", 5.795941, -9.669611, 2.09],
      ["meissa", "觜宿一", 5.585633, 9.934167, 3.39],
      ["c-ori", "伐一", 5.589767, -4.838361, 4.59],
      ["theta1-ori", "伐二", 5.587906, -5.389667, 4.0],
      ["hatysa", "伐三", 5.590550, -5.909889, 2.77],
    ],
    edges: [["meissa", "betelgeuse"], ["meissa", "bellatrix"], ["betelgeuse", "alnitak"], ["bellatrix", "mintaka"],
      ["mintaka", "alnilam"], ["alnilam", "alnitak"], ["alnitak", "saiph"], ["mintaka", "rigel"],
      ["c-ori", "theta1-ori"], ["theta1-ori", "hatysa"]],
  },
  cassiopeia: {
    stars: [
      ["caph", "王良一", 0.152969, 59.149778, 2.27],
      ["schedar", "王良四", 0.675122, 56.537333, 2.24],
      ["navi", "策", 0.945147, 60.716750, 2.47],
      ["ruchbah", "阁道三", 1.430278, 60.235278, 2.68],
      ["segin", "阁道二", 1.906592, 63.670111, 3.37],
    ],
    edges: [["caph", "schedar"], ["schedar", "navi"], ["navi", "ruchbah"], ["ruchbah", "segin"]],
  },
};

// 仅供冒烟核对:北斗「指极星」天璇→天枢的大圆指向北极星。
export const POLARIS = ["polaris", "北极星", 2.530303, 89.264111, 1.98];
