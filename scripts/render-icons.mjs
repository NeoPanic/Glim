// Optional asset generation: install sharp and expose it through NODE_PATH.
// node scripts/render-icons.mjs
// Normal builds use the checked-in PNG/ICNS and do not require Node.js.
import { createRequire } from 'node:module';
import { mkdir, readFile, mkdtemp, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
const require = createRequire(import.meta.url);
const sharp = require('sharp');
const source = await readFile(fileURLToPath(new URL('../assets/branding/glim.svg', import.meta.url)), 'utf8');
// Match standard macOS app artwork: an 824px tile in a 1024px canvas.
const scale = 824 / 960;
const inset = 100 - 32 * scale;
const dockSource = source
  .replace('<path d="M388', '<g transform="translate(512 512) scale(1.22) translate(-512 -512)"><path d="M388')
  .replace('</svg>', '</g></svg>')
  .replace(/(<svg[^>]*>)/, `$1<g transform="translate(${inset} ${inset}) scale(${scale})">`)
  .replace('</svg>', '</g></svg>');
const directory = await mkdtemp(join(tmpdir(), 'glim-icons-'));
try {
  // Keep the whole rounded tile. GPUI samples straight-alpha PNGs linearly,
  // so transparent black RGB would bleed into a dark seam when scaling.
  const welcomeSource = source.replace(/\s*<rect[^>]*stroke="#C7D4D9"[^>]*\/>/, '');
  if (welcomeSource === source) throw new Error('Welcome icon outline adapter needs review');
  const surfaceSource = welcomeSource.replace(/<rect[^>]*fill="url\(#surface\)"[^>]*\/>/,
    '<rect width="1024" height="1024" fill="url(#surface)"/>');
  if (surfaceSource === welcomeSource) throw new Error('Welcome icon surface adapter needs review');
  const { data: welcomePixels, info } = await sharp(Buffer.from(welcomeSource))
    .resize(168, 168).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const surfacePixels = await sharp(Buffer.from(surfaceSource))
    .resize(168, 168).ensureAlpha().raw().toBuffer();
  for (let offset = 0; offset < welcomePixels.length; offset += 4) {
    if (welcomePixels[offset + 3] === 255) continue;
    surfacePixels.copy(welcomePixels, offset, offset, offset + 3);
  }
  await sharp(welcomePixels, { raw: info }).png()
    .toFile(fileURLToPath(new URL('../assets/branding/glim-welcome.png', import.meta.url)));
  await sharp(Buffer.from(source)).resize(1024, 1024).png().toFile(fileURLToPath(new URL('../assets/branding/glim.png', import.meta.url)));
  const iconset = join(directory, 'Glim.iconset');
  await mkdir(iconset);
  for (const size of [16, 32, 128, 256, 512]) {
    for (const density of [1, 2]) {
      const suffix = density === 2 ? '@2x' : '';
      await sharp(Buffer.from(dockSource)).resize(size * density, size * density).png()
        .toFile(join(iconset, `icon_${size}x${size}${suffix}.png`));
    }
  }
  execFileSync('iconutil', ['-c', 'icns', iconset, '-o', fileURLToPath(new URL('../assets/macos/Glim.icns', import.meta.url))]);
} finally {
  await rm(directory, { recursive: true, force: true });
}
