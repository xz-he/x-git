import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DetailPanel from "@/components/layout/DetailPanel.vue";
import SplitDiff from "@/features/history/SplitDiff.vue";
import { useChangesStore } from "@/stores/changes";
import { useUiStore } from "@/stores/ui";
import { useRepositoryStore } from "@/stores/repository";
enableAutoUnmount(afterEach);
beforeEach(() => {
  setActivePinia(createPinia());
  useChangesStore().selectedPath = 'long.md';
  useChangesStore().selectedDiff = { path: 'long.md', scope: 'staged', binary: false, hunks: [{ index: 0, header: '@@ -0,0 +1,844 @@', lines: Array.from({ length: 844 }, (_, i) => ({ kind: 'addition' as const, content: `line ${i + 1}`, oldLine: null, newLine: i + 1 })) }] };
});
describe('single-file fullscreen', () => {
  it('fills the pane, toggles fullscreen, and restores drafts and selected file with Escape', async () => {
    useChangesStore().commitMessage = 'unfinished message';
    useUiStore().contextWidth = 720;
    const wrapper = mount(DetailPanel);
    expect(wrapper.findComponent(SplitDiff).props('fillHeight')).toBe(true);
    await wrapper.get('[aria-label="全屏查看文件差异"]').trigger('click');
    expect(wrapper.classes()).toContain('diff-fullscreen');
    expect(useUiStore().diffFullscreen).toBe(true);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await flushPromises();
    expect(wrapper.classes()).not.toContain('diff-fullscreen');
    expect(useChangesStore().commitMessage).toBe('unfinished message');
    expect(useChangesStore().selectedPath).toBe('long.md');
    expect(useUiStore().contextWidth).toBe(720);
  });
  it('exits when switching repository generation', async () => {
    const wrapper = mount(DetailPanel);
    await wrapper.get('[aria-label="全屏查看文件差异"]').trigger('click');
    useRepositoryStore().generation += 1;
    await flushPromises();
    expect(useUiStore().diffFullscreen).toBe(false);
  });
  it('adapts virtual rows to a tall viewport and disconnects the observer', async () => {
    let resize!: ResizeObserverCallback;
    const disconnect = vi.fn();
    vi.stubGlobal('ResizeObserver', class { constructor(cb: ResizeObserverCallback) { resize = cb; } observe() {} disconnect = disconnect; });
    try {
      const wrapper = mount(SplitDiff, { props: { hunks: useChangesStore().selectedDiff!.hunks, fillHeight: true, rowHeight: 28 } });
      resize([{ contentRect: { height: 1400 } }] as ResizeObserverEntry[], {} as ResizeObserver);
      await flushPromises();
      expect(wrapper.get('[aria-label="修改后"]').text()).toContain('line 50');
      expect(wrapper.findAll('.diff-cell').length).toBeLessThan(180);
      const right = wrapper.get('[aria-label="修改后"]');
      Object.assign(right.element, { scrollTop: 22000 });
      await right.trigger('scroll');
      expect(right.text()).toContain('line 800');
      wrapper.unmount();
      expect(disconnect).toHaveBeenCalled();
    } finally { vi.unstubAllGlobals(); }
  });
});
