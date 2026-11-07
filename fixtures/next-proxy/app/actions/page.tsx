import { InspectForm } from "./form";
import { record, refreshIsr } from "./actions";

export default function Actions() {
  return (
    <main>
      <h1 id="title">actions</h1>
      <InspectForm />
      <form action={async (formData: FormData) => { "use server"; await record(formData); }}>
        <input type="hidden" name="key" value="mpa" />
        <button id="record" type="submit">record</button>
      </form>
      <form action={async () => { "use server"; await refreshIsr(); }}>
        <button id="refresh-isr" type="submit">refresh isr</button>
      </form>
    </main>
  );
}
