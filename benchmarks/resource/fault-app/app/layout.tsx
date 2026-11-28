import type { ReactNode } from "react";

export const metadata = {
  title: "NextRS resource fault app",
  description: "Route handlers for the resource harness's fault workloads",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
