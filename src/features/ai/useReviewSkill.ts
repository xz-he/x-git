import { computed, watch } from "vue";
import { useAiStore } from "@/stores/ai";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";

export function useReviewSkill() {
  const ai = useAiStore();
  const repositories = useRepositoryStore();
  const settings = useSettingsStore();
  watch(() => [repositories.snapshot?.rootPath, repositories.generation, settings.settings.reviewSkillDirectory], () => { void ai.refreshReviewSkill(); }, { immediate: true });
  const ready = computed(() => !ai.skillLoading && ai.reviewSkill?.state === "ready"
    && ai.reviewSkillRoot === repositories.snapshot?.rootPath
    && ai.reviewSkillDirectory === settings.settings.reviewSkillDirectory);
  return { ready };
}
