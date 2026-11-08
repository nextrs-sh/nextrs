export const metadata = { title: "NextRS hybrid" };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" data-router="app">
      <body>{children}</body>
    </html>
  );
}
