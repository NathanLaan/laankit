// Regenerates assets/help-pages.json from the Svelte build's help content
// (apps/noteliner/src/renderer/lib/helpContent.js), so both builds show the
// same Help pages. Run from the repo root after editing helpContent.js:
//
//   node apps/noteliner/native/scripts/gen-help.mjs
//
// A unit test fails when the JSON is out of date.

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, '../../src/renderer/lib/helpContent.js');
const { helpPages } = await import(pathToFileURL(source).href);
const pages = helpPages.map(({ id, title, section, body }) => ({ id, title, section, body }));
writeFileSync(join(here, '../assets/help-pages.json'), JSON.stringify(pages, null, 2) + '\n');
console.log(`wrote ${pages.length} help pages`);
