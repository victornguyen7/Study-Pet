// Asks, on every page opened, whether the pet may note the time spent here. It reads nothing from the page.
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
    const question = el("p", "Study Pet: can I look at this page on ");
    question.append(
      el("strong", location.hostname),
      "?",
      el("small", "I keep the page's address, its title and the time you spend. Nothing else, and never during a break."),
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

  if (window === window.top) {
    chrome.runtime.sendMessage({ type: "check" }, (answer) => {
      if (!chrome.runtime.lastError && answer?.ask) ask();
    });
    // the page changed its address without loading again, or its tab came to the front
    chrome.runtime.onMessage.addListener((message) => {
      if (message.type === "ask") ask();
    });
  }
}
