// ISR with fallback: listed params are prerendered, others are generated on
// their first request and cached.
export const revalidate = 60;

export function generateStaticParams() {
  return [{ slug: "x" }];
}

export default async function IsrPost({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return (
    <main>
      <h1 id="title">ISR post</h1>
      <p id="slug">{slug}</p>
      <p>stamp-{Date.now()}</p>
    </main>
  );
}
