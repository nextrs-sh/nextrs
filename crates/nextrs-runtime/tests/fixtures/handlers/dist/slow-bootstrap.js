// A bootstrap that is one long turn of CPU (as loading large server bundles
// is under contention): longer than the stall limit the tests set, shorter
// than the bootstrap deadline. NEXTRS_TEST_BOOT_MS sets its length; a
// negative value spins forever.
const ms = Number(process.env.NEXTRS_TEST_BOOT_MS ?? 1200);
const end = ms < 0 ? Infinity : Date.now() + ms;
while (Date.now() < end) {
  // spin
}
