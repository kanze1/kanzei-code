// SVG master lives in crates/kanzei-app/web/public/assets/kanzei.svg;
// platform bitmaps are derived by `cargo tauri icon`.
import { access, copyFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const assetsDir = new URL("../crates/kanzei-app/web/public/assets/", import.meta.url);
for (const name of ["kanzei.svg", "kanzei-symbol.svg"]) {
  const target = new URL(name, assetsDir);
  await access(target);
  console.log(fileURLToPath(target));
}

if (process.argv.includes("--platform")) {
  const root = fileURLToPath(new URL("../", import.meta.url));
  const source = "crates/kanzei-app/web/public/assets/kanzei.svg";
  for (const args of [
    ["tauri", "icon", source, "--output", "crates/kanzei-app/icons", "--ios-color", "#ff8700"],
    ["tauri", "icon", source, "--output", "output/brand-export", "--png", "1024", "--png", "512", "--png", "192"],
  ]) {
    const result = spawnSync("cargo", args, { cwd: root, stdio: "inherit", windowsHide: true });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`cargo tauri icon failed (${result.status})`);
  }
  for (const [size, destination] of [
    [1024, "crates/kanzei-app/app-icon.png"],
    [512, "crates/kanzei-app/mobile-pwa/icon-512.png"],
    [192, "crates/kanzei-app/mobile-pwa/icon-192.png"],
  ]) {
    await copyFile(
      new URL(`../output/brand-export/${size}x${size}.png`, import.meta.url),
      new URL(`../${destination}`, import.meta.url),
    );
  }
}
