// Sends the page in front to the Study Pet app, every 5 s, for sites the user said yes to (PR-2).
// It asks once per site and remembers the answer: a yes or no covers every page on that site, in
// every tab, until it is changed or forgotten in the popup. A site with no answer yet is asked about when
// one of its pages loads or comes to the front.
// The app keeps what is sent; Chrome keeps only the list of sites and their answers.

const HOST = "com.hiennguyen.pet"; // HOST_NAME in src-tauri/src/browser.rs
let port = null;
let connected = false; // true once the app has said hello

function connect() {
  if (port) return;
  const p = chrome.runtime.connectNative(HOST);
  port = p;
  p.onMessage.addListener(() => {
    connected = true;
    askFront(); // pages that loaded before the pet was there
  });
  // the app is not running, or it quit
  p.onDisconnect.addListener(() => {
    void chrome.runtime.lastError;
    if (port === p) {
      port = null;
      connected = false;
    }
  });
}

// the address without its #fragment, which is how the app tells pages apart
const page = (url) => Object.assign(new URL(url), { hash: "" }).href;
// "site:<host>" -> true or false, the answer for every page on that site; kept until forgotten
const key = (url) => `site:${new URL(url).hostname}`;
const answer = async (url) => (await chrome.storage.local.get(key(url)))[key(url)];

/** Shows the banner in a tab whose site has no answer yet. */
async function askIfNew(tab) {
  if (!connected || !/^https?:/.test(tab?.url ?? "")) return;
  if ((await answer(tab.url)) !== undefined) return;
  try {
    await chrome.tabs.sendMessage(tab.id, { type: "ask" });
  } catch {
    // a tab from before the extension was loaded has no script; once there, it asks by itself
    chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["content.js"] }).catch(() => {});
  }
}
const askFront = async () => askIfNew((await chrome.tabs.query({ active: true, lastFocusedWindow: true }))[0]);

async function sample() {
  if (!connected) return;
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab?.url || !URL.canParse(tab.url)) return;
  // Chrome is behind another app
  if (!(await chrome.windows.get(tab.windowId)).focused) return;
  if (!/^https?:/.test(tab.url) || (await answer(tab.url)) !== true) return;
  port?.postMessage({ url: page(tab.url), title: tab.title ?? "" });
}

// an open port keeps this worker awake, so the 5 s timer runs for as long as the app does;
// without the app the worker sleeps and the alarm tries again every half minute
connect();
setInterval(sample, 5000);
chrome.alarms.create("connect", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(connect);

// a page that changes its address without loading runs no new content script, so a move to a site
// with no answer is asked about from here
chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.url) askIfNew(tab);
});
// switching to a tab, or back to Chrome, asks about a site that has no answer yet
chrome.tabs.onActivated.addListener(askFront);
chrome.windows.onFocusChanged.addListener(askFront);

// the popup changes the answers in storage directly; one it forgot is asked about again
chrome.storage.onChanged.addListener((changes) => {
  if (Object.values(changes).some((c) => c.newValue === undefined)) askFront();
});

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  (async () => {
    if (message.type === "check") {
      // content.js, on load: only ask while the pet is there to watch
      reply({ ask: connected && (await answer(sender.tab.url)) === undefined });
    } else if (message.type === "answer") {
      // from the banner, about the site its own tab is on
      await chrome.storage.local.set({ [key(sender.tab.url)]: message.allow === true });
      // the same question in other tabs on this site is answered too
      for (const tab of await chrome.tabs.query({ url: `*://${new URL(sender.tab.url).hostname}/*` }))
        chrome.tabs.sendMessage(tab.id, { type: "answered" }).catch(() => {});
      reply({});
    } else if (message.type === "status") {
      reply({ connected });
    }
  })();
  return true; // the reply comes later
});
