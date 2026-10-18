import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import http from "node:http";
import net from "node:net";
import { closeNativeResources } from "./fixtures/native-app.mjs";

const app=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{windowsHide:true,stdio:"ignore"});
await once(app,"spawn");
const model=http.createServer(),probe=net.createServer();
await new Promise(resolve=>model.listen(0,"127.0.0.1",resolve));
await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));
let closedBrowser=false;
const shutdownError=new Error("injected IPC shutdown failure"),browserError=new Error("injected CDP disconnect failure");
await assert.rejects(closeNativeResources({app,model,probe,browser:{async close(){closedBrowser=true;throw browserError;}}},async()=>{throw shutdownError;}),error=>{
  assert(error instanceof AggregateError);
  assert.deepEqual(error.errors,[shutdownError,browserError]);
  return true;
});
assert(closedBrowser);
assert(!model.listening && !probe.listening,"Both real listening servers must close despite independent cleanup failures");
assert(app.exitCode!==null || app.signalCode!==null,"The real owned child must have exited before cleanup returns");
await closeNativeResources({app,model,probe});
console.log("Native fixture lifecycle PASS: failed shutdown and disconnect still release the owned child and both servers; released resources stay idempotent");
