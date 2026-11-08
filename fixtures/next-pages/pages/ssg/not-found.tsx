import type { GetStaticProps } from "next";

export const getStaticProps: GetStaticProps = async () => ({ notFound: true });

export default function Never() {
  return <p>never rendered</p>;
}
