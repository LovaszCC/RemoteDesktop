// The questions an RDP session asks (Swift `CertificatePrompt`, `CredentialPrompt`): accept the server
// certificate (`rdp-cert-prompt`) and the NLA sign-in (`rdp-credentials-prompt`). One dialog at a time, in
// arrival order; `rdp-prompt-closed` (the session ended) drops a prompt, open or queued.

import { onRdpCertPrompt, onRdpCredentialsPrompt, onRdpPromptClosed, type Unlisten } from '../api';
import { errorMessage } from '../errors';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type {
  CertificateChoice,
  RdpCertPromptDto,
  RdpCredentialsAnswer,
  RdpCredentialsPromptDto,
} from '../types';

export type RdpPrompt =
  ({ kind: 'certificate' } & RdpCertPromptDto) | ({ kind: 'credentials' } & RdpCredentialsPromptDto);

export function createRdpPromptsStore(deps: StoreDeps & { alerts?: Pick<AlertStore, 'show'> } = {}) {
  const { api, listen } = resolveDeps(deps);
  const alerts = deps.alerts ?? defaultAlerts;
  let prompts = $state<RdpPrompt[]>([]);
  let unlisten: Unlisten[] = [];

  function drop(requestId: string) {
    prompts = prompts.filter((p) => p.requestId !== requestId);
  }

  function add(prompt: RdpPrompt) {
    if (prompts.some((p) => p.requestId === prompt.requestId)) return;
    prompts = [...prompts, prompt];
  }

  /** Takes the current prompt if it is of `kind`. */
  function take<K extends RdpPrompt['kind']>(kind: K): Extract<RdpPrompt, { kind: K }> | null {
    const prompt = prompts[0];
    if (prompt?.kind !== kind) return null;
    drop(prompt.requestId);
    return prompt as Extract<RdpPrompt, { kind: K }>;
  }

  return {
    /** The prompt shown now, or `null`. */
    get current(): RdpPrompt | null {
      return prompts[0] ?? null;
    },
    get queued(): number {
      return prompts.length;
    },
    async start() {
      if (unlisten.length > 0) return;
      unlisten = [
        await onRdpCertPrompt(listen, (p) => add({ kind: 'certificate', ...p })),
        await onRdpCredentialsPrompt(listen, (p) => add({ kind: 'credentials', ...p })),
        await onRdpPromptClosed(listen, (c) => drop(c.requestId)),
      ];
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
    },
    /** Exposed for tests. */
    add,
    closed: drop,
    /** Answers the current certificate prompt. */
    async answerCertificate(choice: CertificateChoice) {
      const prompt = take('certificate');
      if (prompt) await api.rdpCertAnswer(prompt.requestId, choice).catch(() => {});
    },
    /** Answers the current sign-in prompt; `null` cancels. A failed save shows the alert. */
    async answerCredentials(answer: RdpCredentialsAnswer | null) {
      const prompt = take('credentials');
      if (!prompt) return;
      try {
        await api.rdpCredentialsAnswer(prompt.requestId, answer);
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },
  };
}

export type RdpPromptsStore = ReturnType<typeof createRdpPromptsStore>;

export const rdpPrompts = createRdpPromptsStore();
