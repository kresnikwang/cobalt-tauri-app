// Background service worker: right-click "Send to Cobalt" for pages, links and
// videos. Uses the same `cobalt://` deep link the app already registers.
const COBALT_SCHEME = "cobalt://download?url=";

function openCobalt(url) {
  if (!url || !/^https?:/i.test(url)) return;
  // Opening the custom scheme hands the URL to the OS, which launches Cobalt.
  chrome.tabs.create({ url: COBALT_SCHEME + encodeURIComponent(url), active: false });
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "cobalt-page", title: "Send this page to Cobalt", contexts: ["page"] });
    chrome.contextMenus.create({ id: "cobalt-link", title: "Send link to Cobalt", contexts: ["link"] });
    chrome.contextMenus.create({ id: "cobalt-video", title: "Send video to Cobalt", contexts: ["video"] });
  });
});

chrome.contextMenus.onClicked.addListener((info) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl;
  openCobalt(url);
});
