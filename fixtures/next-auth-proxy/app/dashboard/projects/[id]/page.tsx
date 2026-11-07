import { verifySession } from "../../../lib/dal";

export default async function Project({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const session = await verifySession();
  return (
    <main>
      <h1 id="title">Project</h1>
      <p id="slug">{id}</p>
      <p id="secret">project {id} of {session.name}</p>
    </main>
  );
}
