import type { GetStaticProps } from "next";

export const getStaticProps: GetStaticProps<{ when: string }> = async () => ({ props: { when: "build" } });

export default function PagesSsg({ when }: { when: string }) {
  return <p id="when">{when}</p>;
}
