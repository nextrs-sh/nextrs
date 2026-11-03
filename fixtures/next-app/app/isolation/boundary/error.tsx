"use client";

export default function BoundaryError({ error }: { error: Error & { digest?: string } }) {
  return <p id="boundary">error boundary (digest {error.digest ?? "none"})</p>;
}
