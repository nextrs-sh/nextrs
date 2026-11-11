import App, { type AppContext, type AppProps } from "next/app";
import Link from "next/link";
import { cookieValue, upstream } from "../lib/upstream";

type AppExtra = { app: { user: string | null; theme: string | null; data: number | null; side: string; path: string } };

// Next runs this on the server for the first render and in the browser for
// client navigation (no req then). It also merges the page's own initial
// props (App.getInitialProps).
function NextrsApp({ Component, pageProps, app }: AppProps & AppExtra) {
  return (
    <div data-app="nextrs-app-gip">
      <p id="app-user">{app.user ?? "-"}</p>
      <p id="app-theme">{app.theme ?? "-"}</p>
      <p id="app-data">{app.data === null ? "-" : `data-${app.data > 0 ? "ok" : "none"}`}</p>
      <p id="app-side">{app.side}</p>
      <p id="app-path">{app.path}</p>
      <nav>
        <Link id="to-home" href="/">home</Link>
        <Link id="to-gip" href="/gip?q=nav">gip</Link>
        <Link id="to-ssr" href="/ssr">ssr</Link>
        <Link id="to-ssg" href="/ssg">ssg</Link>
      </nav>
      <Component {...pageProps} />
    </div>
  );
}

NextrsApp.getInitialProps = async (appContext: AppContext) => {
  const initial = await App.getInitialProps(appContext);
  const req = appContext.ctx.req;
  let data: number | null = null;
  if (req) {
    // Async work on the server: the upstream counts these.
    const r = await fetch(`${upstream()}/data?key=app-gip`, { cache: "no-store" }).then((x) => x.json());
    data = r.hit;
  }
  return {
    ...initial,
    app: {
      user: req ? ((req.headers["x-test-user"] as string | undefined) ?? null) : "client",
      theme: req ? cookieValue(req.headers.cookie, "theme") : "client",
      data,
      side: req ? "server" : "client",
      path: appContext.ctx.asPath ?? "-",
    },
  };
};

export default NextrsApp;
