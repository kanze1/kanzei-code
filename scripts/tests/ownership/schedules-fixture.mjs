import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

// Both scheduling tests execute the same production module and dependency contract.
export async function loadSchedules(context, bindings) {
  const source = readFileSync(new URL("../../../crates/kanzei-app/ui/24-schedules.js", import.meta.url), "utf8");
  const module = new vm.SourceTextModule(source, { context });
  const dependencies = new Map();
  await module.link(specifier => {
    assert(Object.hasOwn(bindings, specifier), "Unmapped scheduling dependency: " + specifier);
    if (!dependencies.has(specifier)) {
      const values = bindings[specifier];
      dependencies.set(specifier, new vm.SyntheticModule(Object.keys(values), function () {
        for (const [key, value] of Object.entries(values)) this.setExport(key, value);
      }, { context }));
    }
    return dependencies.get(specifier);
  });
  await module.evaluate();
  return module;
}
