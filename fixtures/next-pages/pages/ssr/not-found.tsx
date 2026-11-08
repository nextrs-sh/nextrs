import type { GetServerSideProps } from "next";

export const getServerSideProps: GetServerSideProps = async () => ({ notFound: true });

export default function Never() {
  return <p>never rendered</p>;
}
