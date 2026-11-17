// Upstream: inspired by storytold/craft-anything at 2e7f276 (MIT OR Apache-2.0)
// - The scenes and the headless flag: crates/ui-egui/src/demo.rs
// - About 40 scenes rendered to PNG: crates/ui-egui/examples/snapshot.rs

import { createServer } from "vite";
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { mkdir } from "node:fs/promises";

const SIZES = [
  { width: 1280, height: 800 },
  { width: 800, height: 1280 },
];

function parseArgs(argv) {
  let only = null;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--only" && i + 1 < argv.length) {
      only = argv[++i];
    } else if (arg.startsWith("--only=")) {
      only = arg.slice("--only=".length);
    }
  }
  return { only };
}

async function main() {
  const { only } = parseArgs(process.argv.slice(2));

  const rootDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const outDir = resolve(rootDir, "scenes-out");
  await mkdir(outDir, { recursive: true });

  const server = await createServer({
    root: rootDir,
    configFile: resolve(rootDir, "vite.config.ts"),
    server: {
      port: 5173,
      strictPort: false,
    },
  });
  await server.listen();

  const address = server.httpServer.address();
  const port = typeof address === "object" && address ? address.port : 5173;
  const baseUrl = `http://localhost:${port}`;

  let browser;
  try {
    const { scenes } = await server.ssrLoadModule(resolve(rootDir, "src/scenes.ts"));
    const sceneNames = only ? [only] : Object.keys(scenes);

    for (const name of sceneNames) {
      if (!scenes[name]) {
        throw new Error(`Scene not found: "${name}". Available scenes: ${Object.keys(scenes).join(", ")}`);
      }
    }

    browser = await chromium.launch();

    for (const scene of sceneNames) {
      for (const { width, height } of SIZES) {
        const page = await browser.newPage({
          viewport: { width, height },
        });
        try {
          await page.goto(`${baseUrl}/?scene=${encodeURIComponent(scene)}`, {
            waitUntil: "domcontentloaded",
          });
          await page.waitForSelector(".app", { state: "visible" });
          await page.evaluate(() => document.fonts?.ready);
          await page.waitForTimeout(100);

          const outFile = resolve(outDir, `${scene}-${width}x${height}.png`);
          await page.screenshot({ path: outFile, fullPage: true });
          console.log(`Wrote ${outFile}`);
        } finally {
          await page.close();
        }
      }
    }
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
