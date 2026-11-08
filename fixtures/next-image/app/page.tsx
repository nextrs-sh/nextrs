import { base } from "./lib/base";
import Image, { getImageProps } from "next/image";
import Link from "next/link";
import still from "./assets/still.jpg";
import wide from "./assets/wide.png";

const port = process.env.NEXTRS_IMAGE_UPSTREAM_PORT ?? "4317";

function ArtDirection() {
  const common = { alt: "art", sizes: "100vw" };
  const {
    props: { srcSet: desktop },
  } = getImageProps({ ...common, width: 1440, height: 875, quality: 50, src: `${base}/photos/photo.jpg` });
  const {
    props: { srcSet: mobile, ...rest },
  } = getImageProps({ ...common, width: 750, height: 1334, quality: 75, src: `${base}/photos/odd.jpg` });
  return (
    <picture>
      <source media="(min-width: 1000px)" srcSet={desktop} />
      <source media="(min-width: 500px)" srcSet={mobile} />
      <img {...rest} id="art" style={{ width: "100%", height: "auto" }} />
    </picture>
  );
}

export default function Home() {
  return (
    <main>
      <h1>next/image</h1>
      <Link id="to-second" href="/second">second page</Link>
      <section>
        <Image id="local" src={`${base}/photos/photo.jpg`} alt="local" width={800} height={600} />
        <Image id="alpha" src={`${base}/photos/alpha.png`} alt="alpha" width={400} height={300} />
        <Image id="static" src={still} alt="static" placeholder="blur" />
        <div style={{ position: "relative", width: "50vw", height: 300 }}>
          <Image id="fill" src={`${base}/photos/odd.jpg`} alt="fill" fill sizes="50vw" style={{ objectFit: "cover" }} />
        </div>
        <Image id="preload" src={`${base}/photos/photo.webp`} alt="preload" width={600} height={400} preload />
        <Image id="quality" src={`${base}/photos/photo.jpg`} alt="quality" width={300} height={225} quality={90} />
        <Image id="unoptimized" src={`${base}/photos/small.png`} alt="unoptimized" width={48} height={32} unoptimized />
        <Image id="remote" src={`http://127.0.0.1:${port}/img/photo.jpg`} alt="remote" width={640} height={480} />
        <Image id="sizes" src={wide} alt="sizes" sizes="(max-width: 600px) 100vw, 33vw" />
        <Image id="svg" src={`${base}/photos/icon.svg`} alt="svg" width={64} height={64} unoptimized />
        <Image id="dynamic" src={`${base}/api/image/dyn.png`} alt="dynamic" width={320} height={240} />
      </section>
      <ArtDirection />
      <div id="spacer" style={{ height: 4000 }} />
      <Image id="lazy" src={`${base}/photos/rotated.jpg`} alt="lazy" width={400} height={600} />
    </main>
  );
}
