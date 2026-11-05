'use server';

type Greeting = { greeting: string } | null;

// Bound with .bind() on the server: the bound argument travels to the
// browser in the action reference and comes back with the call.
export async function greet(prefix: string, _state: Greeting, formData: FormData): Promise<Greeting> {
  return { greeting: `${prefix}, ${String(formData.get('name'))}` };
}
