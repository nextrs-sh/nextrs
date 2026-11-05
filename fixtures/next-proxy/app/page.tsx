import Link from "next/link";

export default function Home() {
  return (
    <main>
      <h1 id="title">home</h1>
      <ul>
        <li><Link id="link-target" href="/target">target</Link></li>
        <li><Link id="link-proxy-rewrite" href="/proxy/rewrite">proxy rewrite</Link></li>
        <li><Link id="link-proxy-redirect" href="/proxy/redirect">proxy redirect</Link></li>
        <li><Link id="link-protected" href="/protected">protected</Link></li>
        <li><Link id="link-dynamic" href="/dynamic">dynamic</Link></li>
        <li><Link id="link-af" href="/af/linked">afterFiles rewrite</Link></li>
        <li><Link id="link-old-blog" href="/old-blog/moved">config redirect</Link></li>
      </ul>
    </main>
  );
}
