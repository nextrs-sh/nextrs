import { ProtectedForm } from "./form";

export default async function ProtectedAction({ searchParams }: { searchParams: Promise<{ key?: string }> }) {
  const { key } = await searchParams;
  return (
    <main>
      <h1 id="title">Protected action</h1>
      <ProtectedForm ledgerKey={key ?? "browser"} />
    </main>
  );
}
