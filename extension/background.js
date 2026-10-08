// Sends the page in front to the Study Pet app, every 5 s, for sites the user said yes to (PR-2).
// It asks once per site and remembers the answer: a yes or no covers every page on that site, in
// every tab, until it is changed or forgotten in the popup. A site with no answer yet is asked about when
// one of its pages loads or comes to the front.
// The app keeps what is sent; Chrome keeps only the list of sites and their answers.

// Each app's host is named after its identifier (start in src-tauri/src/browser.rs). The everyday app
// comes first: the dev build gets the pages only while the everyday app is closed.
const HOSTS = ["com.hiennguyen.pet", "com.hiennguyen.pet.dev"];
let port = null;
let connected = false; // true once the app has said hello

function connect(i = 0) {
  if (port || i >= HOSTS.length) return;
  const p = chrome.runtime.connectNative(HOSTS[i]);
  port = p;
  let hello = false;
  p.onMessage.addListener((message) => {
    // the one thing the app says after hello: block this site for some minutes (drift ladder, step 4)
    if (typeof message?.block === "string") return block(message.block, message.minutes);
    hello = connected = true;
    askFront(); // pages that loaded before the pet was there
  });
  // the app is not running, or it quit
  p.onDisconnect.addListener(() => {
    void chrome.runtime.lastError;
    if (port === p) {
      port = null;
      connected = false;
      if (!hello) connect(i + 1); // that app is not there: try the next
    }
  });
}

// the address without its #fragment, which is how the app tells pages apart
const page = (url) => Object.assign(new URL(url), { hash: "" }).href;
// "site:<host>" -> true or false, the answer for every page on that site; kept until forgotten
const key = (url) => `site:${new URL(url).hostname}`;
const answer = async (url) => (await chrome.storage.local.get(key(url)))[key(url)];

// "block:<host>" -> when the block ends (ms); session storage, so it is gone when Chrome closes
const blockKey = (host) => `block:${host}`;
const blockedUntil = async (url) => {
  const until = (await chrome.storage.session.get(blockKey(new URL(url).hostname)))[blockKey(new URL(url).hostname)];
  return until > Date.now() ? until : undefined;
};
/** Covers every open page on a site until the block ends; pages opened later ask at load ("check"). */
async function block(host, minutes) {
  if (!/^[\w.-]+$/.test(host) || !(minutes > 0 && minutes <= 60)) return;
  const until = Date.now() + minutes * 60000;
  await chrome.storage.session.set({ [blockKey(host)]: until });
  for (const tab of await chrome.tabs.query({ url: `*://${host}/*` }))
    chrome.tabs.sendMessage(tab.id, { type: "block", until }).catch(() => {});
}

/** Shows the banner in a tab whose site has no answer yet. */
async function askIfNew(tab) {
  if (!connected || tab?.incognito || !/^https?:/.test(tab?.url ?? "")) return;
  if ((await answer(tab.url)) !== undefined) return;
  try {
    await chrome.tabs.sendMessage(tab.id, { type: "ask" });
  } catch {
    // a tab from before the extension was loaded has no script; once there, it asks by itself
    chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["content.js"] }).catch(() => {});
  }
}
const askFront = async () => askIfNew((await chrome.tabs.query({ active: true, lastFocusedWindow: true }))[0]);

/** Whether the page in a tab is showing a password field right now. */
async function hasPassword(tabId) {
  try {
    return (await chrome.tabs.sendMessage(tabId, { type: "password?" })) === true;
  } catch {
    // no script in the tab yet (it was open before the extension was loaded): put it in for next time
    chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] }).catch(() => {});
    return false;
  }
}

// Every sample says whether what is in front in Chrome is private (PR-1), and names the page only
// when it is not and its site was allowed.
async function sample() {
  if (!connected) return;
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  // No window this extension can see has the keyboard: Chrome is behind another app, or an incognito
  // window is in front. Only the app can tell which, so it is told "private" and decides.
  const focused = tab && (await chrome.windows.get(tab.windowId)).focused;
  const web = /^https?:/.test(tab?.url ?? "");
  // PR-11: only that some tab is playing, never which one or what
  const audio = (await chrome.tabs.query({ audible: true })).length > 0;
  if (!focused || tab.incognito || (web && (await hasPassword(tab.id)))) return port?.postMessage({ private: true, audio });
  const allowed = web && (await answer(tab.url)) === true;
  port?.postMessage({ private: false, audio, ...(allowed && { url: page(tab.url), title: tab.title ?? "" }) });
}

// an open port keeps this worker awake, so the 5 s timer runs for as long as the app does;
// without the app the worker sleeps and the alarm tries again every half minute
connect();
setInterval(sample, 5000);
chrome.alarms.create("connect", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(() => connect());

// a page that changes its address without loading runs no new content script, so a move to a site
// with no answer is asked about from here
chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.url) askIfNew(tab);
});
// switching to a tab, or back to Chrome, asks about a site that has no answer yet
chrome.tabs.onActivated.addListener(askFront);
// and the app hears at once whether Chrome's front window is one it may see
chrome.windows.onFocusChanged.addListener(() => {
  askFront();
  sample();
});

// the popup changes the answers in storage directly; one it forgot is asked about again
chrome.storage.onChanged.addListener((changes) => {
  if (Object.values(changes).some((c) => c.newValue === undefined)) askFront();
});

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  (async () => {
    if (message.type === "check") {
      // content.js, on load: only ask while the pet is there to watch
      reply({
        ask: connected && !sender.tab.incognito && (await answer(sender.tab.url)) === undefined,
        blockedUntil: await blockedUntil(sender.tab.url),
      });
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
