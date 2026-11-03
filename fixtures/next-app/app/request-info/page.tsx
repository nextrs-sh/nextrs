import { cookies, headers } from "next/headers";

export default async function RequestInfoPage() {
  const headerList = await headers();
  const cookieStore = await cookies();
  const probe = headerList.get("x-nextrs-probe") ?? "(none)";
  const theme = cookieStore.get("theme")?.value ?? "(none)";
  return (
    <main>
      <h1 id="title">Request info</h1>
      <p id="probe">x-nextrs-probe={probe}</p>
      <p id="theme">theme={theme}</p>
    </main>
  );
}
