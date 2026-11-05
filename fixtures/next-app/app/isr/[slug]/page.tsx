import { renderData } from "../../lib/isr";

// Two params prerendered; any other is rendered on its first request and
// then cached like them (dynamicParams defaults to true).
export const revalidate = 3600;

export function generateStaticParams() {
  return [{ slug: "one" }, { slug: "two" }];
}

export default async function Slug({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const data = await renderData(`isr-slug-${slug}`, [`isr-slug-${slug}`]);
  return (
    <main>
      <h1 id="title">{`slug ${slug}`}</h1>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
      <p id="hit">{String(data.hit)}</p>
    </main>
  );
}
