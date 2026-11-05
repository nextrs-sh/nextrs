'use client';

export default function ActionError({ error, reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return (
    <main>
      <h1 id="error-boundary">Error boundary</h1>
      <p id="error-message">{error.message}</p>
      <p id="error-digest">{error.digest ? 'has-digest' : 'no-digest'}</p>
      <button type="button" onClick={reset}>Retry</button>
    </main>
  );
}
