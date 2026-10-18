// Transport boundary: contracts are generated from the registered Rust signatures.
import { commandParameters } from "./00-command-contract.js";
import { recordConsole } from "./00-console-log.js";
const transport = window.__TAURI__.core.invoke;
export function validateCommandArguments(command, args = {}) {
  if (command.startsWith("plugin:")) return;
  const fields = commandParameters[command];
  if (!fields) throw new Error(`Unknown IPC command: ${command}`);
  if (!args || typeof args !== "object" || Array.isArray(args)) throw new Error(`Invalid ${command} arguments`);
  for (const key of Object.keys(args)) if (!(key in fields)) throw new Error(`Unknown ${command} argument: ${key}`);
  for (const [key, required] of Object.entries(fields)) if (required && args[key] === undefined) throw new Error(`Missing ${command} argument: ${key}`);
}
export async function invoke(command, args = {}) {
  validateCommandArguments(command, args);
  try {
    return await transport(command, args);
  } catch (cause) {
    // Arguments may contain credentials or user content; retain only the command.
    recordConsole("ipc-failure", [`IPC ${command}`, cause]);
    throw cause;
  }
}
export const { listen } = window.__TAURI__.event;
