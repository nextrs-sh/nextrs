import type { GetServerSideProps } from "next";

export const getServerSideProps: GetServerSideProps = async () => {
  throw new Error("getServerSideProps failure (synthetic)");
};

export default function Never() {
  return <p>never rendered</p>;
}
