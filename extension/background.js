// Sends the page in front to the Study Pet app, every 5 s, for pages the user said yes to (PR-2).
// It asks on every page and remembers nothing: a yes or no covers that page in that tab, until the
// tab goes somewhere else. A page with no answer yet is asked when it loads or comes to the front.
// The app keeps what is sent; Chrome keeps only the open tabs' answers.

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
// tab id -> { url, allow }, the answer for the page in it; session storage is gone when Chrome closes
const key = (tabId) => `tab${tabId}`;
const answer = async (tabId) => (await chrome.storage.session.get(key(tabId)))[key(tabId)];

/** Shows the banner in a tab whose page has no answer yet. */
async function askIfNew(tab) {
  if (!connected || !/^https?:/.test(tab?.url ?? "")) return;
  if ((await answer(tab.id))?.url === page(tab.url)) return;
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
  const a = await answer(tab.id);
  if (!a?.allow || a.url !== page(tab.url)) return;
  port?.postMessage({ url: page(tab.url), title: tab.title ?? "" });
}

// an open port keeps this worker awake, so the 5 s timer runs for as long as the app does;
// without the app the worker sleeps and the alarm tries again every half minute
connect();
setInterval(sample, 5000);
chrome.alarms.create("connect", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(connect);
// answers an earlier version remembered per site
chrome.runtime.onInstalled.addListener(() => chrome.storage.local.clear());

chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(key(tabId)));
// the tab went to another page: the answer ends. A page that changes its address without loading
// (a next video, say) runs no new content script, so it is told to ask again.
chrome.tabs.onUpdated.addListener(async (tabId, change, tab) => {
  if (!change.url || !URL.canParse(change.url) || (await answer(tabId))?.url === page(change.url)) return;
  await chrome.storage.session.remove(key(tabId));
  askIfNew(tab);
});
// switching to a tab, or back to Chrome, asks about a page that has no answer yet
chrome.tabs.onActivated.addListener(askFront);
chrome.windows.onFocusChanged.addListener(askFront);

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  (async () => {
    if (message.type === "check") {
      // content.js, on load: only ask while the pet is there to watch
      reply({ ask: connected && (await answer(sender.tab.id))?.url !== page(sender.tab.url) });
    } else if (message.type === "answer") {
      // from the banner, about the page its own tab is on
      await chrome.storage.session.set({ [key(sender.tab.id)]: { url: page(sender.tab.url), allow: message.allow === true } });
      reply({});
    } else if (message.type === "status") {
      reply({ connected });
    }
  })();
  return true; // the reply comes later
});
