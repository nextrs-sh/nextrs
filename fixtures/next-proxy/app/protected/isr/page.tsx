export const revalidate = 3600;

export default function ProtectedIsr() {
  return <main><h1 id="title">protected isr</h1><p id="stamp">stamp-{Date.now()}</p><p id="secret">synthetic protected content</p></main>;
}
