import createMiddleware from "next-intl/middleware";
import { routing } from "./i18n/routing";

// next-intl's middleware, exactly as its documentation sets it up.
export default createMiddleware(routing);

export const config = {
  matcher: "/((?!api|trpc|_next|_vercel|.*\\..*).*)",
};
