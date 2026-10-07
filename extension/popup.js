chrome.runtime.sendMessage({ type: "status" }, (s) => {
  document.getElementById("status").textContent = s?.connected
    ? "Connected. The pet asks on every page you open, and notes only the ones you say yes to."
    : "Study Pet is not running, so nothing is asked or noted.";
});
