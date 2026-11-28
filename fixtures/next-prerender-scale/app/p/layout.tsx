// A nested layout, so each route has several RSC segments, as real apps do.
export default function SectionLayout({ children }: { children: React.ReactNode }) {
  return (
    <section>
      <nav>
        <a href="/">Home</a> · <a href="/p/0">First</a>
      </nav>
      {children}
    </section>
  );
}
