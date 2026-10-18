// Questions `ssh` asks that the vault can't answer (`askpass-prompt`): a retried password or passphrase,
// an unknown host key (yes/no), anything else. One dialog at a time, in arrival order; `askpass-closed`
// (the session ended) drops a prompt, open or queued.

import { onAskpassClosed, onAskpassPrompt, type Unlisten } from '../api';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type { AskpassPromptDto } from '../types';

export function createAskpassStore(deps: StoreDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  let prompts = $state<AskpassPromptDto[]>([]);
  let unlisten: Unlisten[] = [];

  function drop(requestId: string) {
    prompts = prompts.filter((p) => p.requestId !== requestId);
  }

  function add(prompt: AskpassPromptDto) {
    if (prompts.some((p) => p.requestId === prompt.requestId)) return;
    prompts = [...prompts, prompt];
  }

  return {
    /** The prompt shown now, or `null`. */
    get current(): AskpassPromptDto | null {
      return prompts[0] ?? null;
    },
    get queued(): number {
      return prompts.length;
    },
    async start() {
      if (unlisten.length > 0) return;
      unlisten = [
        await onAskpassPrompt(listen, add),
        await onAskpassClosed(listen, (c) => drop(c.requestId)),
      ];
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
    },
    /** Exposed for tests. */
    add,
    closed: drop,
    /** Answers the current prompt; `null` refuses (Cancel). */
    async answer(answer: string | null) {
      const prompt = prompts[0];
      if (!prompt) return;
      drop(prompt.requestId);
      await api.askpassAnswer(prompt.requestId, answer).catch(() => {});
    },
  };
}

export type AskpassStore = ReturnType<typeof createAskpassStore>;

export const askpass = createAskpassStore();
