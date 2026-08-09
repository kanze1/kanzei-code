---
id: M-021
scope: project
category: sop
title: edit 报 old_string 匹配多处时先 read 定位并收窄，非批量勿设 replace_all
description: 处理 edit 报“old_string matches N locations”时必读：不要重复提交同一个宽泛 old_string；先 read 当前目标文件并用文件路径、函数/区块边界及邻近行构造唯一上下文，确认仅命中 1 处后再 edit。只有明确要改全部命中时才设 replace_all=true，并先核对每个命中范围。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

处理 edit 报错：`old_string matches 18 locations in C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app/src/update.rs; make it unique with more context, or set replace_all=true.` 时，停止重试原字符串；先 read 当前目标区块，补入文件结构、函数/区块边界和足够邻近行，使 old_string 只命中目标 1 处，再执行 edit。仅在明确的批量替换且已核对所有命中范围时才设置 replace_all=true，不能用它掩盖定位不准。\n[fp:edit|old_string matches locations in make it unique with more context, or set replace]
