import { echoForm } from '../server';
import { EchoForm } from '../ui';

// File inputs: one file, several files, and an empty file input.
export default function UploadPage() {
  return (
    <main>
      <h1>Upload action</h1>
      <EchoForm action={echoForm}>
        <input name="caption" id="caption" defaultValue="cover" />
        <input type="file" name="file" id="file" />
        <input type="file" name="many" id="many" multiple />
        <input type="file" name="none" id="none" />
      </EchoForm>
    </main>
  );
}
