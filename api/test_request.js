import { fetch } from 'undici';

async function run() {
  console.log("Sending API request to Cobalt...");
  try {
    const res = await fetch("http://127.0.0.1:47301", {
      method: "POST",
      headers: {
        "Accept": "application/json",
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        url: "https://www.youtube.com/watch?v=g9FMEEX9IV0",
        videoQuality: "720",
        downloadMode: "auto",
        audioFormat: "best",
        filenameStyle: "pretty",
        youtubeVideoCodec: "h264",
        youtubeHLS: false,
        innertubeClient: "IOS"
      })
    });

    const result = await res.json();
    console.log("API response:", result);

    if (result.status === "tunnel") {
      const tunnelUrl = result.url;
      console.log(`Requesting tunnel URL: ${tunnelUrl}`);
      const tunnelRes = await fetch(tunnelUrl);
      console.log(`Tunnel response status: ${tunnelRes.status}`);
      if (tunnelRes.status !== 200) {
        const text = await tunnelRes.text();
        console.log(`Tunnel response body: ${text}`);
      } else {
        console.log("Tunnel response succeeded!");
      }
    }
  } catch (err) {
    console.error("Test failed:", err);
  }
}

run();
