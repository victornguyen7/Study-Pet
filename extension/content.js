// Asks, once per site, whether the pet may note the time spent here. The one thing it looks at on
// the page is whether a password field is showing, so that such a page is never noted.
// It can arrive twice (at load, and put in by background.js), so the second copy does nothing.
if (!globalThis.studyPetAsks) {
  globalThis.studyPetAsks = true;
  let box;

  // built node by node, with a stylesheet object: sites with a strict content policy (GitHub, YouTube)
  // refuse inline styles and HTML strings
  const css = new CSSStyleSheet();
  css.replaceSync(`
    div { position: fixed; right: 16px; top: 16px; z-index: 2147483647; max-width: 320px; padding: 12px 14px;
          background: #1f2933; color: #fff; border-radius: 10px; font: 14px/1.4 system-ui, sans-serif;
          box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3); }
    p { margin: 0 0 10px; }
    small { display: block; opacity: 0.75; margin-top: 4px; }
    button { font: inherit; padding: 4px 12px; margin-right: 8px; border: 0; border-radius: 6px; cursor: pointer; }
    .yes { background: #2dd4bf; color: #06231f; }`);
  const el = (tag, text = "") => Object.assign(document.createElement(tag), { textContent: text });

  function ask() {
    box?.remove();
    const asked = (box = document.createElement("div"));
    const root = asked.attachShadow({ mode: "closed" });
    root.adoptedStyleSheets = [css];

    const dialog = el("div");
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-label", "Study Pet");
    const question = el("p", "Study Pet: can I look at pages on ");
    question.append(
      el("strong", location.hostname),
      "?",
      el("small", "I keep each page's address, its title and the time you spend. Nothing else, and never during a break. I ask once and remember; see and change your answers from the Study Pet button in the toolbar."),
    );
    const button = (text, allow) => {
      const b = el("button", text);
      if (allow) b.className = "yes";
      b.onclick = () => {
        chrome.runtime.sendMessage({ type: "answer", allow });
        asked.remove();
      };
      return b;
    };
    dialog.append(question, button("Yes", true), button("No", false));
    root.append(dialog);
    document.documentElement.append(asked);
  }

  // Drift ladder, step 4 (opt-in in the app): the page is covered until the block ends.
  let cover;
  function block(until) {
    cover?.remove();
    if (!(until > Date.now())) return;
    const covered = (cover = document.createElement("div"));
    const root = covered.attachShadow({ mode: "closed" });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(`
      div { position: fixed; inset: 0; z-index: 2147483647; display: grid; place-content: center; text-align: center;
            background: #1f2933; color: #fff; font: 20px/1.5 system-ui, sans-serif; }
      small { display: block; opacity: 0.75; font-size: 14px; }`);
    root.adoptedStyleSheets = [sheet];
    const time = new Date(until).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    const message = el("div", `Study Pet is holding ${location.hostname} until ${time}.`);
    message.setAttribute("role", "alertdialog");
    message.append(el("small", "You asked for this in Study Pet's settings. Your focus block is waiting."));
    root.append(message);
    document.documentElement.append(covered);
    setTimeout(() => covered.remove(), until - Date.now());
  }

  if (window === window.top) {
    chrome.runtime.sendMessage({ type: "check" }, (answer) => {
      if (chrome.runtime.lastError) return;
      if (answer?.ask) ask();
      block(answer?.blockedUntil);
    });
    // the page changed its address without loading again, or its tab came to the front
    chrome.runtime.onMessage.addListener((message, _sender, reply) => {
      if (message.type === "ask") ask();
      if (message.type === "answered") box?.remove(); // answered in another tab on this site
      if (message.type === "block") block(message.until);
      // PR-1: a page is private while it shows a password field; only this yes or no leaves the page
      if (message.type === "password?")
        reply([...document.querySelectorAll("input[type=password]")].some((input) => input.checkVisibility()));
    });
  }
}
