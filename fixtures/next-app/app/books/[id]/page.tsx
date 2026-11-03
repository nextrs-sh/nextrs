export default async function BookPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return (
    <main>
      <h1 id="title">Book {id}</h1>
      <p id="param">id={id}</p>
    </main>
  );
}
