'use server';

import { createHash } from 'node:crypto';
import { cookies, headers } from 'next/headers';
import { redirect, notFound } from 'next/navigation';
import { revalidatePath, revalidateTag, updateTag, refresh } from 'next/cache';
import { upstream } from '../lib/upstream';

// Server Actions for the Phase 1D fixtures. Results are plain data so the
// tests can compare them between next start and nextrs.

export type FieldValue = string | { file: string; type: string; size: number; sha256: string };
export type EchoState = { ok: boolean; entries: [string, FieldValue][]; error?: string } | null;

async function describeEntries(formData: FormData): Promise<[string, FieldValue][]> {
  const out: [string, FieldValue][] = [];
  for (const [name, value] of formData.entries()) {
    if (name.startsWith('$ACTION')) continue;
    if (typeof value === 'string') {
      out.push([name, value]);
    } else {
      const bytes = new Uint8Array(await value.arrayBuffer());
      out.push([name, { file: value.name, type: value.type, size: value.size, sha256: createHash('sha256').update(bytes).digest('hex') }]);
    }
  }
  return out;
}

// useActionState: echoes every submitted field (files as name/type/size/hash).
export async function echoForm(_previous: EchoState, formData: FormData): Promise<EchoState> {
  const title = formData.get('title');
  if (title !== null && typeof title === 'string' && title.trim() === '') {
    return { ok: false, entries: [], error: 'Title must not be blank' };
  }
  return { ok: true, entries: await describeEntries(formData) };
}

// A plain Server Function called from client code with ordinary arguments.
export async function whoAmI(tag: string, delayMs = 0) {
  const h = await headers();
  const jar = await cookies();
  if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
  // Read again after the delay: request state must still be this request's.
  const h2 = await headers();
  return {
    tag,
    user: h.get('x-test-user'),
    userAfterDelay: h2.get('x-test-user'),
    cookie: jar.get('nextrs-flavor')?.value ?? null,
  };
}

export async function redirectTo(formData: FormData) {
  const to = String(formData.get('to') ?? '/actions/redirect/done');
  redirect(to);
}

export async function failWith(formData: FormData): Promise<void> {
  const kind = String(formData.get('kind') ?? 'error');
  if (kind === 'not-found') notFound();
  throw new Error(`Action failed on purpose (${kind})`);
}

export async function throwFromClient(message: string): Promise<string> {
  throw new Error(message);
}

export async function setFlavor(formData: FormData) {
  const flavor = String(formData.get('flavor') ?? '');
  const jar = await cookies();
  if (flavor === '') {
    jar.delete('nextrs-flavor');
  } else {
    jar.set('nextrs-flavor', flavor, { path: '/', httpOnly: true, sameSite: 'lax' });
  }
}

// Records one side effect in the test upstream (tests/compat/upstream.mjs),
// which counts them outside every isolate.
export async function recordMutation(key: string, id: string, delayMs = 0, spinMs = 0, waitBeforeMs = 0) {
  if (waitBeforeMs > 0) await new Promise((resolve) => setTimeout(resolve, waitBeforeMs));
  const res = await fetch(upstream(`/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`), { method: 'POST' });
  if (!res.ok) throw new Error(`upstream refused the mutation: ${res.status}`);
  if (spinMs > 0) {
    const until = Date.now() + spinMs;
    while (Date.now() < until) {
      // Busy: holds the isolate's thread after the side effect committed.
    }
  }
  if (delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
  return { key, id, recorded: true };
}

export async function neverResolves(): Promise<never> {
  return new Promise<never>(() => {});
}

export async function largeResult(size: number) {
  return { size, text: 'x'.repeat(Math.min(Math.max(size, 0), 8 * 1024 * 1024)) };
}

export async function invalidate(formData: FormData) {
  const how = String(formData.get('how'));
  const tag = String(formData.get('tag') ?? 'actions-revalidate');
  switch (how) {
    case 'revalidatePath':
      revalidatePath('/actions/revalidate');
      break;
    case 'revalidateStaticPath':
      revalidatePath('/actions/static-time');
      break;
    case 'revalidateTag':
      revalidateTag(tag, 'max');
      break;
    case 'updateTag':
      updateTag(tag);
      break;
    case 'refresh':
      refresh();
      break;
    default:
      throw new Error(`unknown invalidation ${how}`);
  }
}
