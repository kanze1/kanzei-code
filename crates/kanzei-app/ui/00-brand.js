// Shared vector geometry for the application icon, inline brand and live agent mark.
// Export assets with scripts/generate-brand-assets.mjs; no raster images are embedded.
export const AGENT_ROTATIONS = [0, 120, 240];
export const AGENT_MODULE_PATH = "M32 9Q33 9 34.2 9.7L45 15.9Q47 17 47 19.3V26.2Q47 28.3 45.2 29.3L39.1 32.8Q37.7 33.6 36.5 32.8Q35.5 32.2 35.5 30.7V25.6A3.5 3.5 0 0 0 28.5 25.6V30.7Q28.5 32.2 27.5 32.8Q26.3 33.6 24.9 32.8L18.8 29.3Q17 28.3 17 26.2V19.3Q17 17 19 15.9L29.8 9.7Q31 9 32 9Z";
export const AGENT_SIGNAL_PATH = "M37 15.2Q37 14.5 37.7 14.9L41.6 17.1Q42.4 17.6 42.4 18.5V20.2Q42.4 21 41.7 20.6L37.8 18.4Q37 17.9 37 17Z";
export const agentModuleTransform = (angle) => `rotate(${angle} 32 33) translate(7.68 3.18) scale(.76)`;

export function rotateAgentPoint([x, y], degrees) {
  const angle = degrees * Math.PI / 180;
  x = x * .76 + 7.68;
  y = y * .76 + 3.18;
  return [
    Math.round((32 + (x - 32) * Math.cos(angle) - (y - 33) * Math.sin(angle)) * 1000) / 1000,
    Math.round((33 + (x - 32) * Math.sin(angle) + (y - 33) * Math.cos(angle)) * 1000) / 1000,
  ];
}

export function brandSvg({ tile = true } = {}) {
  const modules = AGENT_ROTATIONS.map((angle) => `<path d="${AGENT_MODULE_PATH}" transform="${agentModuleTransform(angle)}"/>`).join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="1024" height="1024" role="img" aria-label="Kanzei parallel agents">`
    + (tile ? '<rect width="64" height="64" rx="13" fill="#ff8700"/>' : "")
    + `<g fill="#191c2b">${modules}</g><path d="${AGENT_SIGNAL_PATH}" transform="${agentModuleTransform(0)}" fill="#39d8f6"/></svg>`;
}
