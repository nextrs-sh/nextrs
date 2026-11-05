import { record } from "../../actions/actions";

// The action posts to this page's path, which the proxy protects.
export default function ProtectedActions() {
  return (
    <main>
      <h1 id="title">protected actions</h1>
      <form action={async (formData: FormData) => { "use server"; await record(formData); }}>
        <input type="hidden" name="key" value="protected" />
        <button id="record" type="submit">record</button>
      </form>
    </main>
  );
}
