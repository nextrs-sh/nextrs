export const answer = 6 * 7;
export async function later(ms) {
  await new Promise((resolve) => setTimeout(resolve, ms));
  return "resolved";
}
