// Prerendered at build time. An action that calls revalidatePath on it
// makes next start render it again; the stamp shows which render is served.
export default function StaticTime() {
  return (
    <main>
      <h1>Static page</h1>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
    </main>
  );
}
