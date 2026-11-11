import type { NextPageContext } from "next";
import Router from "next/router";

// The classic getInitialProps redirect: writeHead on the server, the router
// in the browser.
function Redirect() {
  return <h1 id="title">Redirecting</h1>;
}

Redirect.getInitialProps = async (ctx: NextPageContext) => {
  if (ctx.res) {
    ctx.res.writeHead(307, { Location: "/gip?from=redirect" });
    ctx.res.end();
  } else {
    await Router.replace("/gip?from=redirect");
  }
  return {};
};

export default Redirect;
