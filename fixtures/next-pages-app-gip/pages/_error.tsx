import type { NextPageContext } from "next";

type Props = { statusCode: number; fromServer: boolean };

function ErrorPage({ statusCode, fromServer }: Props) {
  return (
    <>
      <h1 id="title">{`Error ${statusCode}`}</h1>
      <p id="error-side">{fromServer ? "server" : "client"}</p>
    </>
  );
}

ErrorPage.getInitialProps = ({ res, err, req }: NextPageContext): Props => ({
  statusCode: res ? res.statusCode : err ? (err.statusCode ?? 500) : 404,
  fromServer: Boolean(req),
});

export default ErrorPage;
