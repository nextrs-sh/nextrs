import Image from "next/image";

export default function WithImage() {
  return (
    <>
      <h1 id="title">Image page</h1>
      <Image id="swatch" src="/swatch.png" alt="swatch" width={64} height={48} />
    </>
  );
}
