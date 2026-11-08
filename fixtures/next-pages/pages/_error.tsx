import type { NextPageContext } from "next";

// The custom error page (used for statuses without their own page). Its
// getInitialProps is the legacy data API.
function ErrorPage({ statusCode }: { statusCode: number }) {
  return (
    <>
      <h1 id="title">Custom error page</h1>
      <p id="info">status {statusCode}</p>
    </>
  );
}

ErrorPage.getInitialProps = ({ res, err }: NextPageContext) => {
  const statusCode = res ? res.statusCode : err ? (err.statusCode ?? 500) : 404;
  return { statusCode };
};

export default ErrorPage;
