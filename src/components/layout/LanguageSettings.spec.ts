import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { h, nextTick } from 'vue';
import LanguageSettings from './LanguageSettings.vue';
import SettingsDialog from './SettingsDialog.vue';
import AppSidebar from './AppSidebar.vue';
import RemoteDetail from '@/features/remotes/RemoteDetail.vue';
import AppSelect from '@/components/common/AppSelect.vue';
import ConfirmDialog from '@/components/common/ConfirmDialog.vue';
import { createBackendFixture, createTestSettings } from '@/test/backend';
import { setBackendClientForTests, type BackendClient } from '@/lib/backend/client';
import { selectOption } from '@/test/select';
import { useSettingsStore } from '@/stores/settings';
import { useRemotesStore } from '@/stores/remotes';
import { applyLanguage, language } from '@/lib/i18n';

let backend: BackendClient;
beforeEach(() => {
  setActivePinia(createPinia());
  backend = createBackendFixture();
  setBackendClientForTests(backend);
  useSettingsStore();
});
afterEach(() => applyLanguage('zh-CN'));

describe('language preferences', () => {
  it('saves the selected locale and restores it when settings load again', async () => {
    const wrapper = mount(LanguageSettings);
    await selectOption(wrapper, 'Language / 语言', 'en');
    expect(backend.settingsSave).toHaveBeenCalledWith(expect.objectContaining({ language: 'en' }));
    expect(language.value).toBe('en');
    expect(wrapper.text()).toContain('Language');
    vi.mocked(backend.settingsLoad).mockResolvedValue({ settings: createTestSettings({ language: 'en' }) });
    wrapper.unmount();
    setActivePinia(createPinia());
    await useSettingsStore().load();
    expect(language.value).toBe('en');
    expect(document.documentElement.lang).toBe('en');
  });

  it('keeps the previous language if saving fails and supports retry', async () => {
    vi.mocked(backend.settingsSave).mockRejectedValueOnce({ code: 'io', message: 'disk full' });
    const wrapper = mount(LanguageSettings);
    await selectOption(wrapper, 'Language / 语言', 'en');
    expect(language.value).toBe('zh-CN');
    expect(useSettingsStore().settings.language).toBe('zh-CN');
    expect(wrapper.get('[role="alert"]').text()).toContain('语言设置保存失败');
    await selectOption(wrapper, 'Language / 语言', 'en');
    expect(language.value).toBe('en');
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it('preserves language when saving an AI form opened before switching', async () => {
    const wrapper = mount(SettingsDialog);
    await selectOption(wrapper, 'Language / 语言', 'en');
    await wrapper.get('[aria-label="AI settings"]').trigger('click');
    await wrapper.get('[aria-label="Model"]').setValue('my-model');
    await wrapper.get('[aria-label="Save AI settings"]').trigger('click');
    await flushPromises();
    expect(backend.settingsSave).toHaveBeenLastCalledWith(expect.objectContaining({ language: 'en', model: 'my-model' }));
    await wrapper.get('[aria-label="Appearance settings"]').trigger('click');
    await selectOption(wrapper, 'Language / 语言', 'bilingual');
    expect(wrapper.text()).toContain('Theme · 主题');
    expect(wrapper.text()).toContain('System · 跟随系统');
    wrapper.unmount();
  });

  it('updates navigation and remote actions live without modifying names, URLs or command targets', async () => {
    const remotes = useRemotesStore();
    remotes.snapshot = { remotes: [{ name: '获取', fetchUrl: 'https://example.test/中文/代码.git', pushUrl: 'ssh://example.test/中文/代码.git', branches: [] }] };
    remotes.selectedRemoteName = '获取';
    const fetch = vi.spyOn(remotes, 'fetch').mockResolvedValue();
    const wrapper = mount({ render: () => h('div', [h(LanguageSettings), h(AppSidebar), h(RemoteDetail)]) });
    await selectOption(wrapper, 'Language / 语言', 'en');
    expect(wrapper.get('[data-view="history"]').text()).toBe('History');
    expect(wrapper.get('.remote-title h1').text()).toBe('获取');
    expect(wrapper.get('.remote-metadata').text()).toContain('https://example.test/中文/代码.git');
    await wrapper.get('.remote-actions button').trigger('click');
    expect(fetch).toHaveBeenCalledWith('获取');
    await selectOption(wrapper, 'Language / 语言', 'bilingual');
    expect(wrapper.get('[data-view="history"]').text()).toContain('History · 提交记录');
    expect(wrapper.get('.remote-actions button').text()).toBe('Fetch · 获取');
    expect(wrapper.get('.remote-title h1').text()).toBe('获取');
    await selectOption(wrapper, 'Language / 语言', 'zh-CN');
    expect(wrapper.get('[data-view="history"]').text()).toBe('提交记录');
    wrapper.unmount();
  });

  it('updates default dialog and select text without remounting or changing option values', async () => {
    const wrapper = mount({ render: () => h('div', [
      h(AppSelect, { modelValue: '', options: [] }),
      h(ConfirmDialog, { title: 'file 获取.txt', confirmLabel: 'OK' }),
    ]) });
    applyLanguage('en'); await nextTick();
    expect(wrapper.get('[role="combobox"]').text()).toBe('Select an option');
    expect(wrapper.get('[data-action="cancel"]').text()).toBe('Cancel');
    expect(wrapper.get('h2').text()).toBe('file 获取.txt');
    applyLanguage('bilingual'); await nextTick();
    expect(wrapper.get('[data-action="cancel"]').text()).toBe('Cancel · 取消');
    wrapper.unmount();
  });
});
