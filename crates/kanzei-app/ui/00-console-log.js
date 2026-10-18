// Shared bounded diagnostic store; importing it does not install browser hooks.
export const uiConsoleLog = [];
export const UI_CONSOLE_MAX = 200;
export function recordConsole(level, args) {
  if (uiConsoleLog.length >= UI_CONSOLE_MAX) uiConsoleLog.shift();
  uiConsoleLog.push({
    level,
    at: Date.now(),
    text: args.map(a => a instanceof Error ? `${a.message}\n${a.stack ?? ""}` : String(a)).join(" "),
  });
}
