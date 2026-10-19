// Development-only asset generation. Node/npm are not used by the desktop app.
import { build } from 'esbuild';
import { copyFile, mkdir, readdir, readFile, writeFile, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
const here = path.dirname(fileURLToPath(import.meta.url));
const output = path.resolve(here, '../../assets/markdown');
await mkdir(output, { recursive: true });
await rm(path.join(output, 'chunks'), { recursive: true, force: true });
const bundled = await build({
  absWorkingDir: here, entryPoints: ['renderer.js'], bundle: true, minify: true,
  format: 'esm', splitting: true, chunkNames: 'chunks/[name]-[hash]', platform: 'browser', target: ['safari15', 'chrome100'],
  outdir: output, external: ['./plantuml.js', './themes.js', './c4.min.js'], legalComments: 'linked', metafile: true,
  plugins: [{
    name: 'd2-webkit-startup',
    setup(build) {
      build.onLoad({ filter: /d2\/dist\/browser\/index\.js$/ }, async ({ path: sourcePath }) => {
        const source = await readFile(sourcePath, 'utf8');
        const before = 'go.run(result.instance);\n  return self.d2;';
        if (!source.includes(before)) throw new Error('D2 startup adapter needs review');
        // Go's browser runtime can yield before registering its JS API in WebKit.
        const after = 'let failure; go.run(result.instance).catch(error => { failure = error; });\n  for (let i = 0; !self.d2 && i < 500; i++) { if (failure) throw failure; await new Promise(resolve => setTimeout(resolve, 10)); }\n  if (!self.d2) throw new Error("D2 WASM startup timed out");\n  return self.d2;';
        const worker = 'async function hf(){let r=new Blob([pf,If],{type:"text/javascript;charset=utf-8"});return new Worker(URL.createObjectURL(r),{type:"module"})}';
        if (!source.includes(worker)) throw new Error('D2 execution adapter needs review');
        // WebKit workers have too little native stack for this Go WASM build.
        // The private renderer already supplies process isolation and a watchdog.
        const local = 'async function hf(){const bridge={}; const scope=Object.create(globalThis); Object.defineProperty(scope,"onmessage",{value:null,writable:true}); scope.postMessage=data=>queueMicrotask(()=>bridge.onmessage?.({data})); new Function("self",pf+If.replaceAll("export ", ""))(scope); bridge.postMessage=data=>queueMicrotask(()=>scope.onmessage({data})); return bridge;}';
        return { contents: source.replace(before, after).replace(worker, local), loader: 'js' };
      });
    },
  }],
});
// Preserve the official TeaVM engine verbatim: re-minifying its generated
// state-machine labels can produce JavaScript that browsers cannot parse.
for (const name of ['plantuml.js', 'themes.js', 'emoji.js', 'openiconic.js']) {
  await copyFile(path.join(here, 'node_modules/@plantuml/core', name), path.join(output, name));
}
for (const target of [...Object.keys(bundled.metafile.outputs).filter(name => name.endsWith('.js')).map(name => path.resolve(here, name)), path.join(output, 'plantuml.js')]) {
  execFileSync(process.execPath, ['--check', '--input-type=module'], {
    input: await readFile(target),
    stdio: ['pipe', 'pipe', 'pipe'],
  });
}
// Include full license texts for all packages that can be in the bundled JS.
const notices = [];
async function scan(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith('.')) continue;
    const folder = path.join(dir, entry.name);
    if (entry.name.startsWith('@')) { await scan(folder); continue; }
    let pkg;
    try { pkg = JSON.parse(await readFile(path.join(folder, 'package.json'), 'utf8')); }
    catch { continue; }
    const files = (await readdir(folder)).filter(name => /^(licen[sc]e|copying|notice)(\.|$)/i.test(name));
    for (const file of files) {
      try { notices.push(`\n## ${pkg.name} ${pkg.version} (${pkg.license || 'see license'})\n\n${await readFile(path.join(folder, file), 'utf8')}`); }
      catch { /* directories with the same name are not license files */ }
    }
    try { await scan(path.join(folder, 'node_modules')); } catch { /* no nested dependencies */ }
  }
}
await scan(path.join(here, 'node_modules'));
// These engines' npm packages omit their upstream implementation licenses.
for (const file of ['D2-LICENSE.txt', 'D2-THIRD-PARTY-NOTICES.txt']) {
  notices.push(`\n## D2 (@terrastruct/d2 0.1.33) — ${file}\n\n${await readFile(path.join(here, 'licenses', file), 'utf8')}`);
}
const pikchrSource = await readFile(path.join(here, 'node_modules/pikchr-wasm/pikchr/pikchr.c'), 'utf8');
notices.push(`\n## Pikchr implementation (D. Richard Hipp, 0BSD)\n\n${pikchrSource.slice(pikchrSource.indexOf('/*\n** Zero-Clause'), pikchrSource.indexOf('****************************************************************************'))}`);
await writeFile(path.join(output, 'THIRD_PARTY_LICENSES.txt'), notices.join('\n'));

// npm archives may carry CRLF text; keep generated repository assets reproducible.
for (const name of ['renderer.js', 'plantuml.js', 'themes.js', 'emoji.js', 'openiconic.js', 'THIRD_PARTY_LICENSES.txt']) {
  const target = path.join(output, name);
  const text = await readFile(target, 'utf8');
  await writeFile(target, text.replace(/\r\n/g, '\n'));
}
