import type { NextPageContext } from "next";

type Props = { query: string; fromServer: boolean; stamp: number };

function Gip({ query, fromServer }: Props) {
  return (
    <>
      <h1 id="title">Page getInitialProps</h1>
      <p id="page-query">{query}</p>
      <p id="page-side">{fromServer ? "server" : "client"}</p>
    </>
  );
}

Gip.getInitialProps = async (ctx: NextPageContext): Promise<Props> => {
  await new Promise((resolve) => setTimeout(resolve, 5));
  return { query: JSON.stringify(ctx.query), fromServer: Boolean(ctx.req), stamp: Date.now() };
};

export default Gip;
