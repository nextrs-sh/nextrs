'use client';

import { throwFromClient } from '../server';
import { CallButton } from '../ui';

export function ClientErrors() {
  return <CallButton id="throw" label="Throw" call={() => throwFromClient('client-called failure')} />;
}
