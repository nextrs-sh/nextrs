import Link from "next/link";

export default function AppPage() {
  return (
    <>
      <h1 id="title">App Router page</h1>
      <p id="kind">app-static</p>
      <Link href="/pages-page" id="to-pages">To the Pages Router page</Link>{" "}
      <Link href="/pages-items/5" id="to-pages-ssr">To a Pages Router SSR page</Link>
    </>
  );
}
