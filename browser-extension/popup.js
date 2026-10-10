// Popup: send the active tab's URL to Cobalt Desktop via its `cobalt://` scheme.
const COBALT_SCHEME = "cobalt://download?url=";

const urlEl = document.getElementById("url");
const sendBtn = document.getElementById("send");
const statusEl = document.getElementById("status");

function setStatus(text, kind) {
  statusEl.textContent = text || "";
  statusEl.className = "status" + (kind ? " " + kind : "");
}

function isSendable(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

async function init() {
  let tab;
  try {
    [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  } catch (error) {
    setStatus("Could not read the active tab.", "err");
    return;
  }
  const url = tab && tab.url ? tab.url : "";
  urlEl.textContent = url || "No page URL";
  urlEl.title = url;
  sendBtn.disabled = !isSendable(url);
  if (!isSendable(url)) {
    setStatus("This page can't be sent (not an http(s) URL).", "err");
  }
}

// Trigger the OS handler for the `cobalt://` scheme. An anchor click is the
// most compatible way across Chrome and Firefox extension popups.
function openCobalt(url) {
  const anchor = document.createElement("a");
  anchor.href = COBALT_SCHEME + encodeURIComponent(url);
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
}

sendBtn.addEventListener("click", async () => {
  let tab;
  try {
    [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  } catch (error) {
    setStatus("Could not read the active tab.", "err");
    return;
  }
  const url = tab && tab.url ? tab.url : "";
  if (!isSendable(url)) {
    setStatus("This page can't be sent.", "err");
    return;
  }
  sendBtn.disabled = true;
  openCobalt(url);
  setStatus("Sent to Cobalt. If nothing happened, make sure Cobalt is installed and running.", "ok");
  setTimeout(() => window.close(), 900);
});

init();
