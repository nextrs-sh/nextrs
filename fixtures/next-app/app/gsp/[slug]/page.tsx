export function generateStaticParams() {
  return [{ slug: "alpha" }, { slug: "beta" }];
}

export default async function GspPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return (
    <main>
      <h1 id="title">Static param {slug}</h1>
    </main>
  );
}
