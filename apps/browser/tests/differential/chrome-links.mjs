// Loads each corpus page in headless Chrome, scripts off and network off,
// served at its real address so relative links resolve as they did, and
// prints the links Chrome shows, in document order, and the page's visible
// text, as JSON.
//
//   node chrome-links.mjs > chrome.json      (needs google-chrome on PATH)
import { spawn } from "node:child_process";
import { readFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const corpus = join(here, "..", "corpus");
const pages = readFileSync(join(here, "pages.tsv"), "utf8")
  .trim()
  .split("\n")
  .map((line) => line.split("\t"));

const profile = mkdtempSync(join(tmpdir(), "chrome-diff-"));
const chrome = spawn("google-chrome", [
  "--headless=new",
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--disable-gpu",
  "about:blank",
]);
const endpoint = await new Promise((resolve, reject) => {
  let buffered = "";
  chrome.stderr.on("data", (chunk) => {
    buffered += chunk;
    const match = buffered.match(/DevTools listening on (ws:\/\/\S+)/);
    if (match) resolve(match[1]);
  });
  chrome.on("exit", () => reject(new Error("chrome exited")));
});

const socket = new WebSocket(endpoint);
await new Promise((resolve) => socket.addEventListener("open", resolve));
let next = 0;
const waiting = new Map();
const listeners = [];
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && waiting.has(message.id)) {
    waiting.get(message.id)(message);
    waiting.delete(message.id);
  } else {
    for (const listener of listeners) listener(message);
  }
});
const send = (method, params = {}, sessionId) =>
  new Promise((resolve) => {
    const id = ++next;
    waiting.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });

const collect = `(() => {
  const collapse = (s) => s.replace(/\\s+/g, " ").trim();
  const links = [...document.querySelectorAll("a[href]")]
    .filter((a) => /^https?:/.test(a.href))
    .filter((a) => a.checkVisibility())
    .filter((a) => !a.closest("[aria-hidden=true]"))
    .map((a) => {
      let text = collapse(a.innerText);
      if (!text) {
        const img = [...a.querySelectorAll("img")].find((i) => collapse(i.alt));
        text = img ? collapse(img.alt) : "";
      }
      return [text, a.href];
    });
  return { links, text: document.body ? document.body.innerText : "" };
})()`;

const out = {};
for (const [name, address] of pages) {
  const body = readFileSync(join(corpus, `${name}.html`));
  const { result } = await send("Target.createTarget", { url: "about:blank" });
  const { result: attached } = await send("Target.attachToTarget", {
    targetId: result.targetId,
    flatten: true,
  });
  const session = attached.sessionId;
  await send("Emulation.setScriptExecutionDisabled", { value: true }, session);
  await send("Fetch.enable", { patterns: [{ urlPattern: "*" }] }, session);
  await send("Page.enable", {}, session);
  const loaded = new Promise((resolve) => {
    listeners.push(async (message) => {
      if (message.sessionId !== session) return;
      if (message.method === "Fetch.requestPaused") {
        const { requestId, request, resourceType } = message.params;
        if (resourceType === "Document" && request.url.split("#")[0] === address.split("#")[0]) {
          const bytes = Buffer.concat([body]);
          await send(
            "Fetch.fulfillRequest",
            {
              requestId,
              responseCode: 200,
              responseHeaders: [{ name: "Content-Type", value: "text/html" }],
              body: bytes.toString("base64"),
            },
            session,
          );
        } else {
          await send("Fetch.failRequest", { requestId, errorReason: "BlockedByClient" }, session);
        }
      }
      if (message.method === "Page.loadEventFired") resolve();
    });
  });
  await send("Page.navigate", { url: address }, session);
  await loaded;
  const { result: evaluated } = await send(
    "Runtime.evaluate",
    { expression: collect, returnByValue: true },
    session,
  );
  out[name] = evaluated.result.value;
  await send("Target.closeTarget", { targetId: result.targetId });
}
console.log(JSON.stringify(out, null, 1));
socket.close();
chrome.kill();
