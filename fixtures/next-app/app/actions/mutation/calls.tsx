'use client';

import { neverResolves, recordMutation } from '../server';
import { CallButton } from '../ui';

// Exposes the actions the reliability tests call over HTTP (their ids are
// only in this page's worker).
export function MutationCalls() {
  return (
    <>
      <CallButton id="mutate" label="Mutate" call={() => recordMutation('page', `p${Date.now()}`)} />
      <CallButton id="hang" label="Hang" call={() => neverResolves()} />
    </>
  );
}
