// Builds the icon data that ships inside Wren (src/lib/icons/*.json) from the
// npm packages, so the app never loads icons from the internet by itself.
//
//   node scripts/gen-icons.mjs <lucide-static dir> <lucide categories.json> <unicode-emoji-json dir>
//
// lucide-static@1.82.0 (ISC), https://lucide.dev/api/categories (same release),
// unicode-emoji-json@0.9.0 (MIT). Only plain shapes are kept (the same rules as
// the downloaded Tabler pack in src-tauri/src/icons.rs), so the markup is safe
// to place inside Wren's page.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const [lucideDir, categoriesFile, emojiDir] = process.argv.slice(2);
if (!lucideDir || !categoriesFile || !emojiDir) {
  console.error('usage: node scripts/gen-icons.mjs <lucide-static dir> <categories.json> <unicode-emoji-json dir>');
  process.exit(1);
}
const out = new URL('../src/lib/icons/', import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const json = (f) => JSON.parse(readFileSync(f, 'utf8'));

const SHAPES = new Set(['path', 'circle', 'rect', 'line', 'polyline', 'polygon', 'ellipse']);
const GEOMETRY = /^[0-9MmLlHhVvCcSsQqTtAaZzEe.,+\- ]{1,4000}$/;
const NUMBER = /^[0-9.eE+\-]{1,32}$/;
const ATTRS = {
  d: GEOMETRY, points: GEOMETRY,
  cx: NUMBER, cy: NUMBER, r: NUMBER, rx: NUMBER, ry: NUMBER, x: NUMBER, y: NUMBER,
  width: NUMBER, height: NUMBER, x1: NUMBER, y1: NUMBER, x2: NUMBER, y2: NUMBER, opacity: NUMBER,
  fill: /^(none|currentColor)$/, stroke: /^(none|currentColor)$/,
};

/** SVG markup for an icon's shape list, or null if anything is unexpected. */
function body(nodes) {
  let s = '';
  for (const [tag, attrs] of nodes) {
    if (!SHAPES.has(tag)) return null;
    s += `<${tag}`;
    for (const [k, v] of Object.entries(attrs)) {
      const ok = ATTRS[k];
      if (!ok || !ok.test(String(v))) return null;
      s += ` ${k}="${v}"`;
    }
    s += '/>';
  }
  return s.length > 0 && s.length <= 8000 ? s : null;
}

// Lucide: [name, markup, "category category", "tag,tag"]
const lucidePkg = json(join(lucideDir, 'package.json'));
const nodes = json(join(lucideDir, 'icon-nodes.json'));
const tags = json(join(lucideDir, 'tags.json'));
const cats = json(categoriesFile);
const icons = [];
for (const [name, n] of Object.entries(nodes)) {
  const b = body(n);
  if (!b) {
    console.warn('skipped', name);
    continue;
  }
  icons.push([name, b, (cats[name] ?? []).join(' '), (tags[name] ?? []).join(',')]);
}
writeFileSync(join(out, 'lucide.json'), JSON.stringify({ version: lucidePkg.version, icons }));

// Emoji up to Emoji 15.0 (older systems show newer ones as empty boxes): [group, [[emoji, name]]]
const emojiPkg = json(join(emojiDir, 'package.json'));
const groups = json(join(emojiDir, 'data-by-group.json')).map((g) => [
  g.name,
  g.emojis.filter((e) => parseFloat(e.emoji_version) <= 15).map((e) => [e.emoji, e.name]),
]);
writeFileSync(join(out, 'emoji.json'), JSON.stringify({ version: emojiPkg.version, groups }));

// Their licenses ship with Wren (ISC / MIT ask for the notice in every copy).
const notice = [
  `Folder icons: Lucide ${lucidePkg.version} (https://lucide.dev)`,
  readFileSync(join(lucideDir, 'LICENSE'), 'utf8'),
  `Emoji names: unicode-emoji-json ${emojiPkg.version} (https://github.com/muan/unicode-emoji-json), data from Unicode CLDR`,
  readFileSync(join(emojiDir, 'LICENSE'), 'utf8'),
].join('\n\n');
writeFileSync(new URL('../static/icon-licenses.txt', import.meta.url).pathname, notice);
console.log(`lucide ${icons.length} icons, emoji ${groups.reduce((n, g) => n + g[1].length, 0)}`);
