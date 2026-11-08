// next/image does not add basePath to string sources (Next's docs: include
// it yourself); static imports carry it already.
export const base = process.env.NEXTRS_VARIANT === "alt" ? "/base" : "";
