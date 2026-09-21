import { afterEach, describe, expect, it } from 'vitest';
import { computed } from 'vue';
import { applyLanguage, language, normalizeLanguage, t } from './i18n';
import { messages } from './locales/messages';

afterEach(() => applyLanguage('zh-CN'));

describe('interface translations', () => {
  it('switches reactively between Chinese, English and bilingual text', () => {
    const label = computed(() => t('fetch'));
    expect(label.value).toBe('获取');
    applyLanguage('en');
    expect(label.value).toBe('Fetch');
    expect(document.documentElement.lang).toBe('en');
    applyLanguage('bilingual');
    expect(label.value).toBe('Fetch · 获取');
    expect(document.documentElement.dataset.language).toBe('bilingual');
  });

  it('defaults old or unsupported preferences to Chinese', () => {
    for (const value of [undefined, null, '', 'fr', {}]) expect(normalizeLanguage(value)).toBe('zh-CN');
    applyLanguage('unsupported');
    expect(language.value).toBe('zh-CN');
  });

  it('has matching placeholders and real English for every message', () => {
    const placeholders = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
    for (const [key, message] of Object.entries(messages)) {
      expect(message.en.trim(), key).not.toBe('');
      expect(message.en, key).not.toMatch(/\p{Script=Han}/u);
      expect(placeholders(message.en), key).toEqual(placeholders(message.zh));
    }
  });

  it('interpolates data literally without translating or reinterpreting its content', () => {
    applyLanguage('en');
    const name = '获取 · feature/中文-{p0} <tag> $&';
    expect(t('noiseRestoreComplete', { count: name })).toBe(`Restored ${name} files.`);
    applyLanguage('bilingual');
    expect(t('noiseRestoreComplete', { count: name })).toBe(`Restored ${name} files. · 已还原 ${name} 个文件。`);
  });
});
