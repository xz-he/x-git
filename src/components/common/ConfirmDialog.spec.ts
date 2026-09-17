import { flushPromises, mount } from "@vue/test-utils";
import { expect, it, vi } from "vitest";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";

it("focuses cancel and blocks confirmation until enabled", async () => {
  const onConfirm = vi.fn();
  const wrapper = mount(ConfirmDialog, {
    attachTo: document.body,
    props: {
      title: "危险操作",
      confirmLabel: "确认执行",
      confirmDisabled: true,
      danger: true,
      onConfirm,
    },
  });
  await flushPromises();

  expect(document.activeElement?.getAttribute("aria-label")).toBe("取消");
  expect(wrapper.get('[aria-label="确认执行"]').attributes()).toHaveProperty(
    "disabled",
  );
  await wrapper.get('[aria-label="确认执行"]').trigger("click");
  expect(onConfirm).not.toHaveBeenCalled();

  wrapper.unmount();
});
