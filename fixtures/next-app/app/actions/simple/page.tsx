import { createBook } from '../actions';

// The Phase 1D sample page, unchanged except for this type-only cast:
// React 19's types require a form action to return void, so `next build`
// rejects passing createBook (which returns its result) directly. The
// compiled output is the same either way.
const formAction = createBook as unknown as (formData: FormData) => Promise<void>;

export default function Page() {
  return (
    <form action={formAction}>
      <input name="title" required />
      <button type="submit">
        Create Book
      </button>
    </form>
  );
}
