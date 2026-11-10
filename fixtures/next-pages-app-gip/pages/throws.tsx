import type { NextPageContext } from "next";

function Throws() {
  return <h1 id="title">never</h1>;
}

Throws.getInitialProps = async (_ctx: NextPageContext) => {
  throw new Error("page getInitialProps failure (synthetic)");
};

export default Throws;
