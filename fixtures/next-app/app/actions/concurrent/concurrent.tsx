'use client';

import { useState } from 'react';
import { recordMutation, whoAmI } from '../server';

// Fires several Server Function calls at once; each result must carry its
// own tag back.
export function Concurrent() {
  const [result, setResult] = useState('none');
  async function run(count: number) {
    const tags = Array.from({ length: count }, (_, i) => `t${i}`);
    const results = await Promise.all(tags.map((tag, i) => whoAmI(tag, (i % 4) * 20)));
    const mismatched = results.filter((r, i) => r.tag !== tags[i]).length;
    setResult(JSON.stringify({ count, mismatched, tags: results.map((r) => r.tag) }));
  }
  async function mutate(key: string, count: number) {
    const ids = Array.from({ length: count }, (_, i) => `m${i}`);
    const results = await Promise.all(ids.map((id) => recordMutation(key, id)));
    setResult(JSON.stringify({ key, recorded: results.filter((r) => r.recorded).length }));
  }
  return (
    <div>
      <button type="button" id="run-8" onClick={() => run(8)}>Run 8</button>
      <button type="button" id="mutate-8" onClick={() => mutate(`browser-${Date.now()}`, 8)}>Mutate 8</button>
      <pre id="concurrent-result">{result}</pre>
    </div>
  );
}
