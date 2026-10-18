import { createApi, tauriBridge, type Api, type Invoke, type Listen } from '../api';

/** What a store needs from the bridge; tests pass fakes. */
export interface StoreDeps {
  invoke?: Invoke;
  listen?: Listen;
}

export function resolveDeps(deps: StoreDeps = {}): { api: Api; listen: Listen } {
  return { api: createApi(deps.invoke ?? tauriBridge.invoke), listen: deps.listen ?? tauriBridge.listen };
}
