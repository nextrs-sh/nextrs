export const getStaticProps = async () => ({ props: { built: "build" } });

export default function PagesSsg({ built }: { built: string }) {
  return (
    <>
      <h1 id="title">Pages SSG</h1>
      <p id="kind">pages-ssg-{built}</p>
    </>
  );
}
