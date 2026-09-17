import { createBackendFixture } from "@/test/backend";

describe("createBackendFixture", () => {
  it("provides deterministic defaults and accepts overrides", async () => {
    const repositoryRefresh = vi.fn();
    const backend = createBackendFixture({ repositoryRefresh });

    await expect(backend.changesSnapshot("C:/repo")).resolves.toEqual({
      files: [],
      stagedCount: 0,
      unstagedCount: 0,
    });
    expect(backend.repositoryRefresh).toBe(repositoryRefresh);
  });
});
