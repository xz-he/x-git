export interface ModuleRequestToken {
  rootPath: string;
  generation: number;
}

export interface ModuleLifecycle {
  replaceRepository(rootPath: string, generation: number): void;
  begin(rootPath: string, generation: number): ModuleRequestToken;
  accept(token: ModuleRequestToken): boolean;
}

export function createModuleLifecycle(): ModuleLifecycle {
  let activeRootPath: string | undefined;
  let activeGeneration = 0;

  return {
    replaceRepository(rootPath, generation) {
      activeRootPath = rootPath;
      activeGeneration = generation;
    },
    begin(rootPath, generation) {
      return { rootPath, generation };
    },
    accept(token) {
      return (
        token.rootPath === activeRootPath &&
        token.generation === activeGeneration
      );
    },
  };
}
