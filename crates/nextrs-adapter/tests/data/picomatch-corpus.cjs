const pm = require(process.argv[2]);
const pathPatterns = ["**", "/**", "/images/**", "/images/*", "/a/**/b", "/assets/*.png", "/img?.jpg", "/_next/static/**",
  "/x/y.z", "/a-b_c/**", "/images/**/*.jpg", "/a/b", "/a/*/c", "/a/*/*", "/*/b", "/a/b*", "/a/*b", "/a/*.*", "/a/?", "/a/??",
  "/**/b", "/**/x/**", "/a/**/*.png", "/a/**.png/c", "/**/*.png", "/a/**/b/**", "/a/**/**/b", "/a/b/", "/a/*/", "/images/**/thumbs/*", "/media/v1/**",
  "/a/**.png", "/a/x**", "/u/*/avatar.png", "/x.y/**", "/%20/**", "/a+b"];
const hostPatterns = ["example.com", "*.example.com", "**.example.com", "cdn-*.example.com", "img?.example.com", "localhost",
  "*.*.example.com", "**", "*", "s3.*.amazonaws.com", "assets.example.co.uk", "a-b.example.com", "?.example.com"];
const segs = ["", "a", "b", "c", ".", "..", ".git", "images", "thumbs", "x.png", "y.jpg", "img1.jpg", "img12.jpg", "a.b", "-", "_next",
  "static", "x", "y.z", "a-b_c", "v1", "media", "avatar.png", "u", "%20", "a+b", "b1", "1b", "ab", "x.y"];
const inputs = new Set(["", "/", "//"]);
for (const s1 of segs) { inputs.add(`/${s1}`); inputs.add(s1); inputs.add(`/${s1}/`); for (const s2 of segs) { inputs.add(`/${s1}/${s2}`);
  for (const s3 of ["", "b", ".", "x.png", "c", "thumbs", "y.jpg", "..", ".hidden"]) inputs.add(`/${s1}/${s2}/${s3}`); } }
for (const p of ["/a/b/c/b", "/a/x/y/z/b", "/images/2026/11/16/y.jpg", "/images/a/thumbs/x.png", "/images/../etc/passwd", "/images/./x.png", "/a/.b/b", "/a/b/../b", "/images//x.png"]) inputs.add(p);
const hosts = ["example.com", "a.example.com", "a.b.example.com", ".example.com", "example.com.evil", "evilexample.com", "cdn-1.example.com",
  "cdn-.example.com", "img1.example.com", "img12.example.com", "localhost", "localhost.", "s3.us-east-1.amazonaws.com", "s3..amazonaws.com",
  "assets.example.co.uk", "a-b.example.com", "x.example.com", ".a.example.com", "a..example.com", "", "a", "example.com/"];
const paths = [...inputs];
const bits = (re, list) => list.map(i => (re.test(i) ? "1" : "0")).join("");
const out = { generator: "picomatch from next@16.4.6 (next/dist/compiled/picomatch); pathname patterns with { dot: true }, hostnames without options", paths, hosts, cases: [] };
for (const pattern of pathPatterns) out.cases.push({ pattern, dot: true, answers: bits(pm.makeRe(pattern, { dot: true }), paths) });
for (const pattern of hostPatterns) out.cases.push({ pattern, dot: false, answers: bits(pm.makeRe(pattern), hosts) });
console.log(JSON.stringify(out));
