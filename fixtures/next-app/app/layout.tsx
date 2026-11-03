import type { ReactNode } from "react";

export const metadata = {
  title: "NextRS fixture",
  description: "Compatibility fixture for the NextRS runtime",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
