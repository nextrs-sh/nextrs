import type { GetStaticProps } from "next";

// A protected SSG page: its stored HTML and JSON must stay behind the proxy.
export const getStaticProps: GetStaticProps = async () => ({ props: { secret: "static-top-secret" } });

export default function StaticSecret({ secret }: { secret: string }) {
  return (
    <>
      <h1 id="title">Protected static</h1>
      <p id="secret">{secret}</p>
    </>
  );
}
