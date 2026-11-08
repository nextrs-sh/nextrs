import type { GetStaticProps } from "next";

type Props = { built: number; nested: { list: number[]; when: string } };

// getStaticProps without revalidate: built once, served as stored.
export const getStaticProps: GetStaticProps<Props> = async () => ({
  props: { built: Date.now(), nested: { list: [1, 2, 3], when: "build" } },
});

export default function Static({ built, nested }: Props) {
  return (
    <>
      <h1 id="title">SSG static</h1>
      <p id="kind">ssg</p>
      <p id="result">{JSON.stringify(nested)}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
