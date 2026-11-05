'use client';

import { useActionState, useState, useTransition } from 'react';
import { useFormStatus } from 'react-dom';
import type { EchoState } from './server';

// Client pieces shared by the Phase 1D fixture pages. Every observable
// result is rendered into an element with an id the browser tests read.

export function Pending() {
  const { pending } = useFormStatus();
  return <span id="pending">{pending ? 'pending' : 'idle'}</span>;
}

export function EchoForm({
  action,
  children,
  id = 'echo',
}: {
  action: (state: EchoState, formData: FormData) => Promise<EchoState>;
  children: React.ReactNode;
  id?: string;
}) {
  const [state, formAction] = useActionState(action, null);
  const [submits, setSubmits] = useState(0);
  return (
    <form id={id} action={formAction} onSubmit={() => setSubmits((n) => n + 1)}>
      {children}
      <button type="submit" id="submit">Submit</button>
      <Pending />
      <span id="submits">{submits}</span>
      <pre id="result">{state === null ? 'none' : JSON.stringify(state)}</pre>
    </form>
  );
}

export function CallButton<T>({ id, label, call }: { id: string; label: string; call: () => Promise<T> }) {
  const [result, setResult] = useState('none');
  const [pending, startTransition] = useTransition();
  return (
    <div>
      <button
        type="button"
        id={id}
        onClick={() =>
          startTransition(async () => {
            try {
              setResult(JSON.stringify({ ok: await call() }));
            } catch (error) {
              setResult(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }));
            }
          })
        }
      >
        {label}
      </button>
      <span id={`${id}-pending`}>{pending ? 'pending' : 'idle'}</span>
      <pre id={`${id}-result`}>{result}</pre>
    </div>
  );
}
