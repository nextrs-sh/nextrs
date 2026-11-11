import type { GetStaticProps } from "next";

// Regenerated in the background every 3 s and on demand (/api/p/revalidate).
export const getStaticProps: GetStaticProps<{ generated: number }> = async () => ({ props: { generated: Date.now() }, revalidate: 3 });

export default function PagesIsr({ generated }: { generated: number }) {
  return <p id="generated">{generated}</p>;
}
