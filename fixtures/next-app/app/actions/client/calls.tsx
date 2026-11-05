'use client';

import { largeResult, whoAmI } from '../server';
import { CallButton } from '../ui';

export function ClientCalls() {
  return (
    <>
      <CallButton id="who" label="Who am I" call={() => whoAmI('client')} />
      <CallButton id="large" label="Large" call={async () => (await largeResult(200_000)).text.length} />
    </>
  );
}
