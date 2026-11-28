// Writes a Server Action spec for the load generator: Velasia's "find a
// booking" form (`lookupBookingAction`), submitted the way a browser
// without JavaScript submits it (multipart form POST carrying React's
// hidden `$ACTION_*` fields, answered with the server-rendered page).
//
//   node discover-action.mjs http://STOREFRONT:8080 SPECS_DIR
//
// Safe to repeat at any rate: the request carries no guest or account
// credentials, so the action validates the reference, finds no owner to
// ask the retailing API about, and renders its "not found" state. Nothing
// is written anywhere, and every response is the same. The hidden fields
// are read from the live page, so the spec matches the build under test.
import { writeFileSync } from "node:fs";

const [base, specs] = process.argv.slice(2);
const page = await (await fetch(`${base}/manage-booking`)).text();
const form = page.match(/<form[^>]*data-testid="lookup-form"[\s\S]*?<\/form>/)?.[0];
if (!form) throw new Error("no lookup form on /manage-booking");
const decode = (s) => s.replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">");
const fields = [...form.matchAll(/<input type="hidden" name="([^"]+)"(?: value="([^"]*)")?\/?>/g)]
  .map(([, name, value]) => [name, decode(value ?? "")]);
if (!fields.some(([name]) => name.startsWith("$ACTION_"))) throw new Error("no action fields in the form");
fields.push(["reference", "VEL-9Z8Y7X"]); // well-formed, owned by nobody

const boundary = "----nextrsResourceBenchmark7d3f";
const body = fields
  .map(([name, value]) => `--${boundary}\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`)
  .join("") + `--${boundary}--\r\n`;
const request = {
  path: "/manage-booking",
  method: "POST",
  headers: [["content-type", `multipart/form-data; boundary=${boundary}`]],
  body,
  // The action's own failure code, rendered into the page's action state.
  expect_body: ["not-found"],
  kind: "server-action",
};
// Check the spec once against the live storefront before writing it.
const check = await fetch(`${base}${request.path}`, {
  method: "POST",
  headers: { ...Object.fromEntries(request.headers), "accept-encoding": "identity" },
  body,
});
const text = await check.text();
if (check.status !== 200 || !text.includes("not-found")) {
  throw new Error(`action check failed: ${check.status}, not-found ${text.includes("not-found")}`);
}
// Identity, body-checked (correctness pass); gzip as browsers ask for it
// (load phases; the load generator does not decompress, so status only).
writeFileSync(`${specs}/action-identity.json`, JSON.stringify({ requests: [request] }, null, 1));
const gzip = { ...request, headers: [...request.headers, ["accept-encoding", "gzip"]], expect_body: [] };
writeFileSync(`${specs}/action-gzip.json`, JSON.stringify({ requests: [gzip] }, null, 1));
console.log(`action specs: ${fields.length - 1} action fields, check ${check.status}, ${text.length} bytes`);
