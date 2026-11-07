import { Info } from "../../lib/info";

export default async function HasGate({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return <main><h1 id="title">has-gate</h1><p id="slug">{slug}</p><Info /></main>;
}
