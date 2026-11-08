import type { NextPageContext } from "next";

type Props = { from: string; query: string };

// Legacy getInitialProps on a page.
function Gip({ from, query }: Props) {
  return (
    <>
      <h1 id="title">getInitialProps</h1>
      <p id="kind">{from}</p>
      <p id="query">{query}</p>
    </>
  );
}

Gip.getInitialProps = async ({ req, query }: NextPageContext): Promise<Props> => ({
  from: req ? "server" : "client",
  query: JSON.stringify(query),
});

export default Gip;
