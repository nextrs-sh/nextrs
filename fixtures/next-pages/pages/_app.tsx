import type { AppProps } from "next/app";
import Link from "next/link";
import "../styles/globals.css";

// The custom App: a shared layout around every page, and global CSS.
export default function App({ Component, pageProps }: AppProps) {
  return (
    <div id="app-shell" data-app="nextrs-pages-app" className="marker-global-css">
      <nav id="nav">
        <Link href="/" id="link-home">Home</Link> <Link href="/about" id="link-about">About</Link>{" "}
        <Link href="/books/42?ref=nav" id="link-book">Book 42</Link> <Link href="/posts/alpha" id="link-post">Post alpha</Link>{" "}
        <Link href="/ssg/static" id="link-ssg">SSG</Link> <Link href="/isr" id="link-isr">ISR</Link>{" "}
        <Link href="/ssr/redirect" id="link-ssr-redirect">SSR redirect</Link>{" "}
        <Link href="/ssr/not-found" id="link-ssr-not-found">SSR notFound</Link>
      </nav>
      <Component {...pageProps} />
    </div>
  );
}
