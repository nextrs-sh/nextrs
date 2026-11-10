import type { GetServerSideProps } from "next";
import { fromContext, LocaleInfo, type LocaleProps } from "../../lib/locale";

type Props = LocaleProps & { id: string; stamp: number };

// A dynamic SSR route: params decoded by Next, locale from routing.
export const getServerSideProps: GetServerSideProps<Props> = async (context) => {
  const id = String(context.params?.id);
  if (id === "missing") return { notFound: true };
  if (id === "moved") return { redirect: { destination: "/about", permanent: false } };
  return { props: { ...fromContext(context), id, stamp: Date.now() } };
};

export default function Item(props: Props) {
  return (
    <>
      <h1 id="title">Item {props.id}</h1>
      <p id="kind">ssr-dynamic</p>
      <p id="slug">{props.id}</p>
      <LocaleInfo props={props} />
    </>
  );
}
