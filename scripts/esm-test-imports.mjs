import { parse } from "espree";

// Collect the real ESM grammar, including side-effect imports and reexports.
// VM fixtures supply explicit dependency implementations; regex-only discovery
// silently omitted zero-binding imports and could not link otherwise valid code.
export function collectEsmImports(sources, implementedModules) {
  const imported = new Map();
  for (const source of sources.values()) {
    const ast = parse(source, { ecmaVersion: "latest", sourceType: "module" });
    for (const item of ast.body) {
      if (!item.source || !["ImportDeclaration", "ExportNamedDeclaration", "ExportAllDeclaration"].includes(item.type)) continue;
      const specifier = item.source.value;
      if (implementedModules.has(specifier)) continue;
      const names = imported.get(specifier) ?? new Set();
      for (const binding of item.specifiers ?? []) {
        if (binding.type === "ImportDefaultSpecifier") names.add("default");
        else if (binding.type === "ImportSpecifier") names.add(binding.imported.name ?? binding.imported.value);
        else if (binding.type === "ExportSpecifier") names.add(binding.local.name ?? binding.local.value);
      }
      imported.set(specifier, names);
    }
  }
  return imported;
}
