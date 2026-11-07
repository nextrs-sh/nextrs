export const revalidate = 30;

// ISR: the cached page is still behind the proxy.
export default function Reports() {
  return (
    <main>
      <h1 id="title">Reports</h1>
      <p id="secret">quarterly reports (ISR, proxy-protected)</p>
      <p>stamp-{Date.now()}</p>
    </main>
  );
}
