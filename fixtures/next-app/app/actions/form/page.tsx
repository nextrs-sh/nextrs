import { echoForm } from '../server';
import { EchoForm } from '../ui';

// Text fields: several, repeated, optional, empty and Unicode values.
export default function FormPage() {
  return (
    <main>
      <h1>Form action</h1>
      <EchoForm action={echoForm}>
        <input name="title" id="title" defaultValue="" />
        <input name="author" id="author" defaultValue="" />
        <label><input type="checkbox" name="tag" value="sf" id="tag-sf" /> sf</label>
        <label><input type="checkbox" name="tag" value="classic" id="tag-classic" /> classic</label>
        <input name="note" id="note" />
        <input name="empty" id="empty" defaultValue="" />
      </EchoForm>
    </main>
  );
}
