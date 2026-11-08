// A page whose component throws during server rendering.
export const getServerSideProps = async () => ({ props: {} });

export default function Broken(): never {
  throw new Error("render failure (synthetic)");
}
