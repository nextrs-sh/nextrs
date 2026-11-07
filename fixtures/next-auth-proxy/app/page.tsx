import Link from "next/link";

export default function Home() {
  return (
    <main>
      <h1 id="title">Home</h1>
      <nav>
        <Link id="to-dashboard" href="/dashboard">Dashboard</Link> <Link id="to-account" href="/account">Account</Link>{" "}
        <Link id="to-reports" href="/reports">Reports</Link> <Link id="to-protected-action" href="/actions/protected">Action</Link>
      </nav>
    </main>
  );
}
