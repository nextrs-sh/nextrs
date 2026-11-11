import type { AppProps } from "next/app";

export default function App({ Component, pageProps }: AppProps) {
  return (
    <div data-app="nextrs-i18n-app">
      <Component {...pageProps} />
    </div>
  );
}
