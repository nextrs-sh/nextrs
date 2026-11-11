import { headers } from "next/headers";
import Image from "next/image";

// Rendered per request (headers() opts out of prerendering): the token the
// soak driver sent must come back, and only it.
export default async function SoakSsr() {
  const token = (await headers()).get("x-soak-token") ?? "none";
  return (
    <main>
      <p id="token">{token}</p>
      <Image src="/photos/photo.jpg" alt="ssr" width={320} height={240} />
    </main>
  );
}
