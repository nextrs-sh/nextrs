import type { GetServerSideProps } from "next";

type Props = { id: string; token: string };

// The token the driver sent must come back, and only it (page and data route).
export const getServerSideProps: GetServerSideProps<Props> = async (context) => ({
  props: { id: String(context.params?.id), token: (context.req.headers["x-soak-token"] as string | undefined) ?? "none" },
});

export default function PagesSsr({ id, token }: Props) {
  return <p id="token">{`${id}:${token}`}</p>;
}
