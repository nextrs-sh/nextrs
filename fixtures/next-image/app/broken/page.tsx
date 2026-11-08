import { base } from "../lib/base";
import Image from "next/image";

// Sources the optimizer must refuse: a missing file and text with an image
// extension. The SVG is rendered unoptimized by next/image itself (it skips
// the optimizer for .svg sources without dangerouslyAllowSVG).
export default function Broken() {
  return (
    <main>
      <Image id="missing" src={`${base}/photos/missing.jpg`} alt="missing" width={200} height={100} />
      <Image id="invalid" src={`${base}/photos/not-an-image.png`} alt="invalid" width={200} height={100} />
      <Image id="svg-optimized" src={`${base}/photos/icon.svg`} alt="svg" width={64} height={64} />
    </main>
  );
}
