export function generateStaticParams() {
  return [{ slug: "hello" }];
}

export default async function Blog({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  return <main><h1 id="title">blog</h1><p id="slug">{slug}</p></main>;
}
