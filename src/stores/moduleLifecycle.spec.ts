import { createModuleLifecycle } from "@/stores/moduleLifecycle";

describe("module lifecycle", () => {
  it("rejects a request token after the repository generation changes", () => {
    const lifecycle = createModuleLifecycle();
    lifecycle.replaceRepository("C:/one", 1);
    const first = lifecycle.begin("C:/one", 1);

    lifecycle.replaceRepository("C:/two", 2);

    expect(lifecycle.accept(first)).toBe(false);
    expect(lifecycle.accept(lifecycle.begin("C:/two", 2))).toBe(true);
  });
});
