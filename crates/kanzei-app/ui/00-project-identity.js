// Windows project identity shared by navigation, resources and durable drafts.
export const pathKey = value => String(value || "").replaceAll("\\", "/").replace(/^\/\/\?\//, "").replace(/\/$/, "").toLowerCase();
export const sameProject = (a, b) => Boolean(a && b) && pathKey(a) === pathKey(b);
