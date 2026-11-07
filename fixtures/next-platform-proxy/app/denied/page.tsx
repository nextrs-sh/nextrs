export default async function Denied({ searchParams }: { searchParams: Promise<Record<string, string>> }) {
  const query = await searchParams;
  return (
    <main>
      <h1 id="title">Denied</h1>
      <p id="query">{JSON.stringify(query)}</p>
    </main>
  );
}
