import { base } from "../lib/base";
import Image from "next/image";
import Link from "next/link";

export default function Second() {
  return (
    <main>
      <h1 id="second-title">second</h1>
      <Link id="to-home" href="/">home</Link>
      <Image id="second-local" src={`${base}/photos/odd.jpg`} alt="second" width={500} height={333} />
    </main>
  );
}
