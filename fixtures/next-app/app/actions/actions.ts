'use server';

export async function createBook(formData: FormData) {
  const title = formData.get('title');

  if (typeof title !== 'string') {
    throw new Error('Invalid title');
  }

  return {
    success: true,
    title,
  };
}
