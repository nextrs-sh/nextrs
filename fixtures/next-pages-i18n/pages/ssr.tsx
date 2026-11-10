import type { GetServerSideProps } from "next";
import { cookieValue } from "../lib/upstream";
import { fromContext, LocaleInfo, type LocaleProps } from "../lib/locale";

type Props = LocaleProps & { query: Record<string, string | string[]>; user: string | null; resolvedUrl: string; stamp: number };

export const getServerSideProps: GetServerSideProps<Props> = async (context) => {
  context.res.setHeader("x-ssr-locale", context.locale ?? "-");
  return {
    props: {
      ...fromContext(context),
      query: context.query as Record<string, string | string[]>,
      user: cookieValue(context.req.headers.cookie, "user"),
      resolvedUrl: context.resolvedUrl,
      stamp: Date.now(),
    },
  };
};

export default function Ssr(props: Props) {
  return (
    <>
      <h1 id="title">SSR {props.locale}</h1>
      <p id="kind">ssr</p>
      <p id="query">{JSON.stringify(props.query)}</p>
      <p id="secret">{`user=${props.user ?? "-"}`}</p>
      <p id="resolved">{props.resolvedUrl}</p>
      <LocaleInfo props={props} />
    </>
  );
}
