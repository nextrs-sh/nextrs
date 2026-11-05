import { invalidateBooks } from "./actions";

export default function Page() {
  return (
    <main>
      {["updateTag", "revalidateTag"].map((how) => (
        <form action={invalidateBooks} key={how}>
          <input type="hidden" name="how" value={how} />
          <button type="submit" id={how}>{how}</button>
        </form>
      ))}
    </main>
  );
}
