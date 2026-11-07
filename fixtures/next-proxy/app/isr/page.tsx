export const revalidate = 3600;

export default function Isr() {
  return <main><h1 id="title">isr</h1><p id="stamp">stamp-{Date.now()}</p></main>;
}
