<script setup lang="ts">
import { computed } from "vue";
import MarkdownIt from "markdown-it";

const props = defineProps<{ content: string }>();
const markdown = new MarkdownIt({ html: false, breaks: true, linkify: false });
// Model output must not navigate to executable/local URLs or load remote images.
markdown.validateLink = (url) => /^https?:\/\//i.test(url);
markdown.renderer.rules.image = (tokens, index) => markdown.utils.escapeHtml(tokens[index]?.content ?? "");
markdown.renderer.rules.link_open = (tokens, index, options, _env, renderer) => {
  tokens[index]!.attrSet("target", "_blank");
  tokens[index]!.attrSet("rel", "noopener noreferrer");
  return renderer.renderToken(tokens, index, options);
};
const html = computed(() => markdown.render(props.content));
</script>

<template>
  <article class="review-markdown" v-html="html" />
</template>

<style scoped>
.review-markdown { min-width: 0; color: var(--text); line-height: 1.7; font-size: 13px; user-select: text; overflow-wrap: anywhere; }
.review-markdown :deep(h1), .review-markdown :deep(h2), .review-markdown :deep(h3) { margin: 1.2em 0 .6em; line-height: 1.4; }
.review-markdown :deep(h1) { font-size: 20px; }
.review-markdown :deep(h2) { font-size: 17px; }
.review-markdown :deep(h3) { font-size: 15px; }
.review-markdown :deep(p) { margin: .6em 0; }
.review-markdown :deep(pre) { overflow-x: auto; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-muted); }
.review-markdown :deep(code) { font-family: var(--font-code); font-size: .95em; white-space: pre-wrap; }
.review-markdown :deep(pre code) { white-space: pre; }
.review-markdown :deep(blockquote) { margin-left: 0; padding-left: 12px; border-left: 3px solid var(--border); color: var(--text-muted); }
.review-markdown :deep(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
.review-markdown :deep(th), .review-markdown :deep(td) { padding: 6px 10px; border: 1px solid var(--border); }
.review-markdown :deep(a) { color: var(--primary); text-decoration: underline; }
.review-markdown :deep(hr) { border: 0; border-top: 1px solid var(--border); margin: 18px 0; }
</style>
