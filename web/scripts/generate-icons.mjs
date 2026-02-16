import sharp from "sharp";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const iconsDir = resolve(__dirname, "../public/icons");
const svgPath = resolve(iconsDir, "icon-512.svg");
const svg = readFileSync(svgPath);

const sizes = [
  { name: "icon-192.png", size: 192 },
  { name: "icon-512.png", size: 512 },
  { name: "apple-touch-icon.png", size: 180 },
];

for (const { name, size } of sizes) {
  await sharp(svg).resize(size, size).png().toFile(resolve(iconsDir, name));
  console.log(`Generated ${name} (${size}x${size})`);
}
