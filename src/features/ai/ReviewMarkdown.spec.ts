import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import ReviewMarkdown from "./ReviewMarkdown.vue";

describe("review Markdown", () => {
  it("renders report headings, lists, code, tables and emphasis", () => {
    const wrapper = mount(ReviewMarkdown, { props: { content: "## 审查摘要\n\n**P1**：检查边界\n\n- 修复建议\n\n```ts\nconst value = '<tag>';\n```\n\n| 文件 | 结论 |\n| --- | --- |\n| a.ts | 需修改 |" } });
    expect(wrapper.get("h2").text()).toBe("审查摘要");
    expect(wrapper.get("strong").text()).toBe("P1");
    expect(wrapper.get("li").text()).toBe("修复建议");
    expect(wrapper.get("pre code").text()).toContain("'<tag>'");
    expect(wrapper.get("td").text()).toBe("a.ts");
  });

  it("escapes HTML and blocks executable URLs, local links and remote images", () => {
    const wrapper = mount(ReviewMarkdown, { props: { content: '<script>alert(1)</script>\n<img src=x onerror=alert(1)>\n\n[bad](javascript:alert(1)) [local](file:///C:/secret) ![tracking](https://example.test/image.png) [docs](https://example.test)' } });
    expect(wrapper.find("script").exists()).toBe(false);
    expect(wrapper.find("img").exists()).toBe(false);
    expect(wrapper.findAll("a")).toHaveLength(1);
    expect(wrapper.get("a").attributes()).toMatchObject({ href: "https://example.test", target: "_blank", rel: "noopener noreferrer" });
    expect(wrapper.text()).toContain("<script>");
    expect(wrapper.text()).toContain("tracking");
  });
});
