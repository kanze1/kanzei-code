import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MarkdownView } from "./MarkdownView";
import { MessageItem } from "../views/chat/MessageItem";
import { parseBlocks } from "./markdownBlocks";

afterEach(cleanup);

it("流式嵌套折叠保留用户展开状态，围栏内的标签保持原文", () => {
  const prefix = "<details><summary>证据</summary>\n<details>\n<summary>子项</sum";
  const { container, rerender } = render(<MarkdownView streaming text={prefix} />);
  const outer = container.querySelector("details")!;
  expect(outer.open).toBe(false);
  expect(container.textContent).not.toContain("</sum");
  fireEvent.click(screen.getByText("证据"));
  const literal = ["\x60\x60\x60html", "<details>示例</details>", "\x60\x60\x60"].join("\n");
  rerender(<MarkdownView text={prefix + "mary>\n" + literal + "\n</details></details>"} />);
  expect(outer.open).toBe(true);
  expect(container.querySelectorAll("details")).toHaveLength(2);
  expect(container.querySelector("code")?.textContent).toBe("<details>示例</details>");
  expect(container.querySelector("details details summary")?.textContent).toBe("子项");
});

it("代码预览只占 14 行，复制和全文按钮保留全部代码", () => {
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
  const lines = Array.from({ length: 24 }, (_, i) => "行 " + (i + 1));
  const { container } = render(
    <MarkdownView text={["\x60\x60\x60c++", ...lines, "\x60\x60\x60"].join("\n")} />,
  );
  expect(container.querySelector("code")?.textContent).toBe(lines.slice(0, 14).join("\n"));
  fireEvent.click(screen.getByRole("button", { name: "复制" }));
  expect(copy).toHaveBeenCalledWith(lines.join("\n"));
  fireEvent.click(screen.getByRole("button", { name: /展开完整代码/ }));
  expect(container.querySelector("code")?.textContent).toBe(lines.join("\n"));
});

it("保留多级列表顺序，表格按需展开，长章节默认可读", () => {
  const table = ["|编号|", "|---|", ...Array.from({ length: 13 }, (_, i) => "|" + i + "|")].join("\n");
  const { container } = render(
    <MarkdownView
      text={"## 总结\n" + "文".repeat(2010) + "\n## 明细\n- 一级\n  3. 二级\n    - 三级\n### 表格\n" + table}
    />,
  );
  expect(container.querySelector("ul > li > ol[start='3'] > li > ul")?.textContent).toContain("三级");
  expect(container.querySelectorAll("details.md-section[open]")).toHaveLength(3);
  expect(container.querySelector(".md-outline")?.textContent).toContain("本条提纲");
  expect(container.querySelectorAll("tbody tr")).toHaveLength(8);
  fireEvent.click(screen.getByRole("button", { name: "显示全部 13 行" }));
  expect(container.querySelectorAll("tbody tr")).toHaveLength(13);
  expect(parseBlocks("~~~\n<details>literal</details>\n~~~")[0]).toMatchObject({ kind: "code" });
});

it("长用户输入只折叠显示，展开后保留原文", () => {
  const text = Array.from({ length: 12 }, (_, i) => "log " + i).join("\n");
  const { container } = render(
    <MessageItem
      message={{ role: "user", parts: [{ type: "text", text }] }}
      globalIndex={1}
      toolResultMap={new Map()}
    />,
  );
  expect(container.querySelector(".msg-user-bubble")?.textContent).not.toContain("log 11");
  fireEvent.click(screen.getByRole("button", { name: /展开全部/ }));
  expect(container.querySelector(".msg-user-bubble")?.textContent).toContain(text);
});
