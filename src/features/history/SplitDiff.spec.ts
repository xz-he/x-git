import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import type { DiffHunk } from "@/lib/backend/types";
import SplitDiff from "./SplitDiff.vue";

describe("split diff viewer", () => {
  it("renders old and new code in separate panes with aligned empty cells", () => {
    const hunks: DiffHunk[] = [{ index: 0, header: "@@ -4 +4,2 @@", lines: [
      { kind: "deletion", content: "old", oldLine: 4, newLine: null },
      { kind: "addition", content: "new", oldLine: null, newLine: 4 },
      { kind: "addition", content: "extra", oldLine: null, newLine: 5 },
    ] }];
    const wrapper = mount(SplitDiff, { props: { hunks } });
    expect(wrapper.get('[aria-label="修改前"]').text()).toContain("old");
    expect(wrapper.get('[aria-label="修改前"]').text()).not.toContain("extra");
    expect(wrapper.get('[aria-label="修改后"]').text()).toContain("extra");
    expect(wrapper.findAll('.diff-cell.empty')).toHaveLength(1);
    wrapper.unmount();
  });

  it("keeps long files bounded and synchronizes both axes when scrolled", async () => {
    const hunks: DiffHunk[] = [{ index: 0, header: "@@ -1,10000 +1,10000 @@", lines: Array.from({ length: 10000 }, (_, i) => ({ kind: "context", content: `line ${i + 1}`, oldLine: i + 1, newLine: i + 1 })) }];
    const wrapper = mount(SplitDiff, { props: { hunks } });
    expect(wrapper.findAll('.diff-cell').length).toBeLessThan(160);
    const left = wrapper.get('[aria-label="修改前"]');
    const right = wrapper.get('[aria-label="修改后"]');
    Object.assign(right.element, { scrollTop: 110000, scrollLeft: 120 });
    await right.trigger("scroll");
    expect(left.element.scrollTop).toBe(110000);
    expect(left.element.scrollLeft).toBe(120);
    expect(right.text()).toContain("line 5000");
    expect(wrapper.findAll('.diff-cell').length).toBeLessThan(160);
    wrapper.unmount();
  });
});
