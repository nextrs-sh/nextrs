// Application errors that escape every handler, after the response:
//   ?kind=uncaught   a timer callback throws
//   ?kind=rejection  a promise rejects with no handler
// next start logs both and keeps serving (Next installs process error
// handlers from its route module); a runtime must not restart for them.
export async function GET(request: Request) {
  const kind = new URL(request.url).searchParams.get("kind");
  if (kind === "uncaught") {
    setTimeout(() => {
      throw new Error("nextrs fixture: uncaught exception from a timer");
    }, 5);
  } else if (kind === "rejection") {
    Promise.reject(new Error("nextrs fixture: unhandled rejection"));
  }
  return Response.json({ kind });
}
