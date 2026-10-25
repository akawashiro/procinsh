#!/usr/bin/env node
// Chromium load generator with the larger default external site list.
process.env.WORKLOAD = 'chromium';
if (!process.env.SITE_LIST && !process.argv.slice(2).length) {
  process.env.SITE_LIST = new URL('./chromium_sites.txt', import.meta.url).pathname;
}
process.env.TAB_COUNT ??= '20';
await import('./browser_workload.mjs');
