// Prerendered at build time: only the proxy stands in front of it.
export default function Account() {
  return (
    <main>
      <h1 id="title">Account</h1>
      <p id="secret">account settings (prerendered, proxy-protected)</p>
    </main>
  );
}
