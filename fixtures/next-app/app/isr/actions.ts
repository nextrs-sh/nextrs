'use server';

import { revalidatePath, revalidateTag, updateTag } from 'next/cache';

// Server Actions for the ISR fixtures. Each records its mutation in the
// test upstream (outside every isolate) and then revalidates.
async function record(id: string) {
  const base = process.env.NEXTRS_UPSTREAM;
  if (!base) return;
  await fetch(`${base}/mutations?key=isr-mutations&id=${encodeURIComponent(id)}`, { method: 'POST' });
}

export async function isrInvalidate(formData: FormData) {
  const how = String(formData.get('how'));
  await record(String(formData.get('id') ?? how));
  switch (how) {
    case 'revalidatePath':
      revalidatePath('/isr/tagged');
      break;
    case 'revalidateTag':
      revalidateTag('isr-tagged', 'max');
      break;
    case 'updateTag':
      updateTag('isr-tagged');
      break;
    case 'none':
      break;
    default:
      throw new Error(`unknown invalidation ${how}`);
  }
}

export async function isrBound(label: string, formData: FormData) {
  await record(`bound:${label}:${String(formData.get('id') ?? '')}`);
  revalidatePath('/isr/tagged');
}
