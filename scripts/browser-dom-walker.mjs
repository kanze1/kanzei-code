// UI2-0926 #8:browser 工具 dom 动作的 DOM 结构 walker —— 唯一源,两个后端共用。
//   - 无头后端:browser-helper.mjs 以 ESM import,交给 playwright 的 page.evaluate 在页面里执行;
//   - 面板后端:Rust 侧(kanzei-tools browser_tool.rs)include_str! 读本文件原文,去掉 export
//     关键字后包进 CDP Runtime.evaluate 表达式。
// 所以本文件只能导出这一个自包含函数:函数体不得引用模块里的其它符号,也不得 import 任何东西。
export function domWalker(sel) {
  const roots = sel ? Array.from(document.querySelectorAll(sel)) : [document.body];
  const seen = new Set();
  const walk = (el, depth) => {
    if (!el || depth > 12 || seen.has(el)) return [];
    seen.add(el);
    const node = {
      tag: el.tagName ? el.tagName.toLowerCase() : "#text",
      id: el.id || undefined,
      cls:
        el.className && typeof el.className === "string"
          ? el.className.split(/\s+/).filter(Boolean).slice(0, 5)
          : undefined,
      text: el.childElementCount === 0 ? (el.textContent || "").trim().slice(0, 80) : undefined,
      // 表单当前值(输入后不一定反映在 textContent 里);密码框不回显。
      value:
        (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT") &&
        el.type !== "password" &&
        el.value
          ? String(el.value).slice(0, 80)
          : undefined,
      children: [],
    };
    for (const child of el.children) {
      node.children.push(...walk(child, depth + 1));
    }
    return [node];
  };
  const out = [];
  for (const root of roots) out.push(...walk(root, 0));
  return JSON.stringify(out);
}
