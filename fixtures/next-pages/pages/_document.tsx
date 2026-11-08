import { Html, Head, Main, NextScript } from "next/document";

// The custom Document: attributes the suites look for in every HTML page.
export default function Document() {
  return (
    <Html lang="en" data-doc="nextrs-pages-doc">
      <Head>
        <meta name="nextrs-document" content="custom" />
      </Head>
      <body data-body="nextrs">
        <Main />
        <NextScript />
      </body>
    </Html>
  );
}
