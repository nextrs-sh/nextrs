// Prerendered at build time, never revalidated (served by NextRS from the
// build output). Payload sizes are close to Velasia's median page: about
// 20-25 KB of HTML and 10-15 KB of RSC per route.
export const dynamicParams = false;

export async function generateStaticParams() {
  const routes = Number(process.env.NEXTRS_PRERENDER_ROUTES ?? 100);
  return Array.from({ length: routes }, (_, i) => ({ id: String(i) }));
}

const words = ["departure", "arrival", "terminal", "gate", "seat", "fare", "baggage", "lounge", "crew", "cabin", "route", "schedule"];

function rows(seed: number) {
  const out = [];
  let x = seed * 2654435761 + 1;
  for (let i = 0; i < 90; i++) {
    x = (x * 1103515245 + 12345) % 2147483648;
    const a = words[x % words.length];
    const b = words[(x >> 4) % words.length];
    out.push({ key: i, code: `VA${(x % 9000) + 1000}`, text: `${a} ${b} ${x % 997}`, price: (x % 50000) / 100 });
  }
  return out;
}

export default async function Route({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <main>
      <h1 id="title">Route {id}</h1>
      <table>
        <tbody>
          {rows(Number(id)).map((r) => (
            <tr key={r.key}>
              <td>{r.code}</td>
              <td>{r.text}</td>
              <td>{r.price.toFixed(2)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </main>
  );
}
