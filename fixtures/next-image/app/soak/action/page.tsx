import { soakEcho } from "./actions";

export default function SoakAction() {
  return (
    <form action={async () => { "use server"; await soakEcho("form"); }}>
      <button type="submit">echo</button>
    </form>
  );
}
