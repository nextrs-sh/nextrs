import Link from "next/link";

export default function PagesPage() {
  return (
    <>
      <h1 id="title">Pages Router page</h1>
      <p id="kind">pages-static</p>
      <Link href="/app-page" id="to-app">To the App Router page</Link>{" "}
      <Link href="/app-items/3" id="to-app-dynamic">To App item 3</Link>{" "}
      <Link href="/pages-items/9" id="to-pages-ssr">To Pages item 9</Link>
    </>
  );
}
