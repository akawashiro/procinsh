import {spawnSync} from 'node:child_process';
import {readdirSync, readFileSync, writeFileSync, rmSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
process.chdir(fileURLToPath(new URL('../', import.meta.url)));
const manifest = 'dist/web/build-manifest.json';
rmSync(manifest, {force: true});
const result = spawnSync(process.execPath, ['node_modules/typescript/bin/tsc', '-p', 'tsconfig.json'], {stdio: 'inherit'});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const files = ['package.json', 'package-lock.json', 'tsconfig.json', 'scripts/build-web.mjs'];
function collect(dir) {
  for (const entry of readdirSync(dir, {withFileTypes: true})) {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) {
      if (entry.name !== 'vendor') collect(path);
    } else if (/\.(ts|html|css)$/.test(entry.name)) files.push(path);
  }
}
collect('src/web');
for (const name of readdirSync('dist/web')) if (name.endsWith('.js')) files.push(`dist/web/${name}`);
// Exact contents avoid timestamp assumptions in checkouts and published archives.
writeFileSync(manifest, JSON.stringify({version: 1, files: Object.fromEntries(files.sort().map(path => [path, readFileSync(path, 'utf8')]))}));
