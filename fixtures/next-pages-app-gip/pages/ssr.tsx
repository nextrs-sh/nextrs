import type { GetServerSideProps } from "next";
import { cookieValue } from "../lib/upstream";

type Props = { theme: string | null; stamp: number };

export const getServerSideProps: GetServerSideProps<Props> = async (context) => ({
  props: { theme: cookieValue(context.req.headers.cookie, "theme"), stamp: Date.now() },
});

export default function Ssr({ theme }: Props) {
  return (
    <>
      <h1 id="title">SSR</h1>
      <p id="page-theme">{theme ?? "-"}</p>
    </>
  );
}
