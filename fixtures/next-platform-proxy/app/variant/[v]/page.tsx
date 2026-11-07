export default async function Variant({ params }: { params: Promise<{ v: string }> }) {
  const { v } = await params;
  return (
    <main>
      <h1 id="title">Variant</h1>
      <p id="slug">{v}</p>
    </main>
  );
}
