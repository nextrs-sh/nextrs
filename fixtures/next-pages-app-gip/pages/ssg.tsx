import type { GetStaticProps } from "next";

export const getStaticProps: GetStaticProps<{ when: string }> = async () => ({ props: { when: "build" } });

export default function Ssg({ when }: { when: string }) {
  return (
    <>
      <h1 id="title">SSG</h1>
      <p id="page-when">{when}</p>
    </>
  );
}
