export const getServerSideProps = async () => ({ props: { secret: "pages-top-secret" } });

export default function PagesSecret({ secret }: { secret: string }) {
  return (
    <>
      <h1 id="title">Pages secret</h1>
      <p id="secret">{secret}</p>
    </>
  );
}
