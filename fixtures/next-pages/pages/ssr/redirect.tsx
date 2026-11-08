import type { GetServerSideProps } from "next";

export const getServerSideProps: GetServerSideProps = async ({ query }) => {
  if (query.permanent === "1") return { redirect: { destination: "/about", permanent: true } };
  if (query.external === "1") return { redirect: { destination: "https://example.com/elsewhere", permanent: false } };
  if (query.status === "303") return { redirect: { destination: "/books/7", statusCode: 303 } };
  return { redirect: { destination: "/about?from=ssr", permanent: false } };
};

export default function Never() {
  return <p>never rendered</p>;
}
