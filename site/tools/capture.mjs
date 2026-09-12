/* Photograph the real STLibrarian UI, running against the real Rust server.
 *
 *   node capture.mjs <admin-password>
 *
 * The app is served by stlibrarian-server on 127.0.0.1:3999 against a
 * throwaway library at /volume1/3dprints, so every pixel is the actual
 * application rendering actual database rows -- no mockups, no doctored DOM.
 * The library path is deliberately the one the app's own placeholder suggests,
 * because a screenshot showing a build directory is a screenshot of a test rig.
 *
 * Three shots, because three is what the page has room to show properly:
 * the library, one model measured, and the print history. The sidebar admin
 * panels are deliberately not photographed -- they are a path and a button,
 * and they put the server's own data directory on screen.
 *
 * Shots land in out/ as 2x PNG; convert.sh downsamples them for the web.
 */
import pkg from "/root/.npm/_npx/db89d7302a373f10/node_modules/playwright-core/index.js";
import { mkdirSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

const { chromium } = pkg;
const BASE = process.env.STLIBRARIAN_URL || "http://127.0.0.1:3999";
const PASSWORD = process.argv[2];
if (!PASSWORD) throw new Error("usage: node capture.mjs <admin-password>");

const OUT = new URL("./out/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch({
  args: ["--no-sandbox", "--force-color-profile=srgb", "--font-render-hinting=none"],
});
const page = await (await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
})).newPage();

page.on("pageerror", (e) => console.error("  [page error]", e.message));

const shot = async (name) => {
  await page.screenshot({ path: `${OUT}${name}.png` });
  console.log(`  captured ${name}.png`);
};

/**
 * Scroll a heading to the top of the inspector column, and nothing else.
 *
 * Deliberately NOT scrollIntoView: that walks every scrollable ancestor, and
 * the model grid is its own scroll container, so it ends up stopped half way
 * through a row of cards -- a screenshot of somebody mid-scroll rather than of
 * the application. Resetting window afterwards does not undo it, because the
 * window never moved. Adjusting the aside's own scrollTop leaves the grid
 * composed exactly as the hero shot has it.
 */
async function revealInInspector(text) {
  await page.evaluate((t) => {
    const aside = document.querySelector("aside.inspector");
    if (!aside) throw new Error("no inspector open");
    const target = [...aside.querySelectorAll("*")].find(
      (e) => (e.textContent || "").trim().startsWith(t) && e.children.length <= 2,
    );
    if (!target) throw new Error(`no "${t}" section in the inspector`);
    aside.scrollTop +=
      target.getBoundingClientRect().top - aside.getBoundingClientRect().top - 12;
  }, text);
  await sleep(1200);
}

// ---- sign in ---------------------------------------------------------------
console.log("signing in");
await page.goto(BASE, { waitUntil: "networkidle" });
await page.getByLabel("Username").fill("admin");
await page.getByLabel("Password").fill(PASSWORD);
await page.getByRole("button", { name: /sign in|log in|continue/i }).click();

// Thumbnails are rendered client-side and uploaded, so let the queue drain --
// a grid full of placeholders is a screenshot of a loading state. Already
// cached from an earlier run, but a cold data directory needs the full wait.
await page.waitForSelector("text=/spur-gear-18t-m2/i", { timeout: 30000 });
console.log("  signed in, rendering thumbnails");
await sleep(14000);
await page.evaluate(() => window.scrollTo(0, 0));
await sleep(1000);

await shot("library");

// ---- a model open: 3D viewer + measured details ----------------------------
console.log("model detail");
await page.getByText("spur-gear-18t-m2", { exact: true }).first().click();
await sleep(4500);
await page.evaluate(() => window.scrollTo(0, 0));
await sleep(500);
await shot("model");

// ---- print history ---------------------------------------------------------
// knurled-knob-m6 has two runs logged against it, so the history is not a
// one-row table that says nothing about the feature -- and it sits in the
// first row of the grid, which matters: Playwright scrolls an element into
// view before clicking it, and the grid is its own scroll container, so
// clicking anything further down leaves the cards stopped mid-row behind an
// otherwise good screenshot.
console.log("print history");
await page.getByText("knurled-knob-m6", { exact: true }).first().click();
await sleep(3000);
await revealInInspector("Prints");
await shot("prints");

await browser.close();
console.log("done");
