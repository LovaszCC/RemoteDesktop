// Unknown host keys of background connections: one dialog at a time, in arrival order. By default the
// port forwards' `host-key-prompt` (with `host-key-closed` dropping a prompt, open or queued); a Files
// window passes its own source (`files-host-key-prompt` for its browser). Answers go to `host_key_answer`.

import { onHostKeyClosed, onHostKeyPrompt, type Listen, type Unlisten } from '../api';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type { HostKeyQuestion } from '../types';

/** What the host key dialog needs from a store: the prompt shown now and how to answer it. */
export interface HostKeyPrompts {
  readonly current: HostKeyQuestion | null;
  answer(accept: boolean): Promise<void>;
}

/** Where the prompts come from. */
export interface HostKeySource {
  prompts(listen: Listen, add: (prompt: HostKeyQuestion) => void): Promise<Unlisten>;
  closed?(listen: Listen, drop: (requestId: string) => void): Promise<Unlisten>;
}

const forwardsSource: HostKeySource = {
  prompts: (listen, add) => onHostKeyPrompt(listen, add),
  closed: (listen, drop) => onHostKeyClosed(listen, (c) => drop(c.requestId)),
};

export function createHostKeyStore(deps: StoreDeps = {}, source: HostKeySource = forwardsSource) {
  const { api, listen } = resolveDeps(deps);
  let prompts = $state<HostKeyQuestion[]>([]);
  let unlisten: Unlisten[] = [];

  function drop(requestId: string) {
    prompts = prompts.filter((p) => p.requestId !== requestId);
  }

  function add(prompt: HostKeyQuestion) {
    if (prompts.some((p) => p.requestId === prompt.requestId)) return;
    prompts = [...prompts, prompt];
  }

  return {
    /** The prompt shown now, or `null`. */
    get current(): HostKeyQuestion | null {
      return prompts[0] ?? null;
    },
    get queued(): number {
      return prompts.length;
    },
    async start() {
      if (unlisten.length > 0) return;
      unlisten = [await source.prompts(listen, add)];
      if (source.closed) unlisten.push(await source.closed(listen, drop));
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
    },
    /** Exposed for tests. */
    add,
    closed: drop,
    /** Accepts or rejects the current prompt. */
    async answer(accept: boolean) {
      const prompt = prompts[0];
      if (!prompt) return;
      drop(prompt.requestId);
      await api.hostKeyAnswer(prompt.requestId, accept).catch(() => {});
    },
  };
}

export type HostKeyStore = ReturnType<typeof createHostKeyStore>;

export const hostKeys = createHostKeyStore();
