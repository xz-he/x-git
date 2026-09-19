import { flushPromises, type DOMWrapper } from "@vue/test-utils";

/** Exercise the same teleported options that the user clicks. */
export async function selectOption(wrapper: Pick<DOMWrapper<Element>, "get">, label: string, value: string | number) {
  const trigger = wrapper.get(`[aria-label="${label}"]`);
  if (trigger.attributes("aria-expanded") !== "true") await trigger.trigger("click");
  await flushPromises();
  const list = document.getElementById(trigger.attributes("aria-controls")!);
  const option = [...(list?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])].find(option => option.dataset.value === String(value));
  if (!option) throw new Error(`Missing option ${String(value)} in ${label}`);
  option.click(); await flushPromises();
}
