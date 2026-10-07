chrome.runtime.sendMessage({ type: "status" }, (s) => {
  document.getElementById("status").textContent = s?.connected
    ? "Connected. The pet asks once per site, and notes only the sites you say yes to."
    : "Study Pet is not running, so nothing is asked or noted.";
});

// Every answer, as background.js keeps it: "site:<host>" -> true or false. A change here is the
// answer from the next 5 s sample on; Forget makes the pet ask again.
const PREFIX = "site:";
const el = (tag, text = "") => Object.assign(document.createElement(tag), { textContent: text });

async function show() {
  const answers = await chrome.storage.local.get(null);
  const keys = Object.keys(answers).filter((k) => k.startsWith(PREFIX)).sort();
  const rows = keys.map((key) => {
    const site = key.slice(PREFIX.length);
    const li = el("li");
    const name = el("span", site);
    name.title = site;
    const allow = el("select");
    allow.append(new Option("Yes", "yes"), new Option("No", "no"));
    allow.value = answers[key] ? "yes" : "no";
    allow.setAttribute("aria-label", `May the pet look at ${site}?`);
    allow.onchange = () => chrome.storage.local.set({ [key]: allow.value === "yes" });
    const forget = el("button", "Forget");
    forget.setAttribute("aria-label", `Forget the answer for ${site}`);
    forget.onclick = async () => {
      await chrome.storage.local.remove(key);
      show();
    };
    li.append(name, allow, forget);
    return li;
  });
  document.getElementById("sites").replaceChildren(...(rows.length ? rows : [el("li", "None yet.")]));
}
show();
