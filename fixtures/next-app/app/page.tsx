import Link from "next/link";
import { Counter } from "./counter";

export default function Home() {
  return (
    <main>
      <h1 id="title">NextRS static fixture</h1>
      <p>This page has no dynamic inputs and should be prerendered at build time.</p>
      <Counter />
      <ul>
        <li><Link href="/dynamic">Dynamic server component</Link></li>
        <li><Link href="/books/42">Book 42</Link></li>
        <li><Link href="/request-info">Request info</Link></li>
      </ul>
    </main>
  );
}
