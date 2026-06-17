import { Innertube, Platform } from 'youtubei.js';
import { Agent, fetch as undiciFetch, setGlobalDispatcher, EnvHttpProxyAgent, ProxyAgent, Request, request } from 'undici';

async function run() {
  // Set proxy env variables
  process.env.HTTP_PROXY = 'http://127.0.0.1:7897';
  process.env.HTTPS_PROXY = 'http://127.0.0.1:7897';
  process.env.NO_PROXY = 'localhost,127.0.0.1,::1';

  // Set the global dispatcher to EnvHttpProxyAgent
  setGlobalDispatcher(new EnvHttpProxyAgent());

  const fetchFn = (input, init) => {
    const url = typeof input === 'string'
              ? new URL(input)
              : input instanceof URL
                ? input
                : new URL(input.url);

    const requestObj = new Request(
        url,
        input instanceof Platform.shim.Request
        ? input : undefined
    );

    return undiciFetch(requestObj, {
      ...init
    });
  };

  const yt = await Innertube.create({ fetch: fetchFn });
  console.log("Getting basic info with IOS client...");
  const info = await yt.getBasicInfo('g9FMEEX9IV0', { client: 'IOS' });
  
  const formats = info.streaming_data.adaptive_formats;
  const videoFormat = formats.find(f => f.has_video);
  const videoUrl = videoFormat.url;
  
  console.log(`Video URL: ${videoUrl.substring(0, 120)}...`);
  
  console.log(`\n--- Test: Fetching using undici.request with global EnvHttpProxyAgent ---`);
  try {
    const res = await request(videoUrl, {
      method: 'GET',
      headers: {
        'user-agent': "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        'accept': '*/*',
        'Range': 'bytes=0-999'
      }
    });
    console.log(`Response status: ${res.statusCode}`);
    if (res.statusCode !== 200 && res.statusCode !== 206) {
      const body = await res.body.text();
      console.log(`Response body: ${body.substring(0, 200)}`);
    }
  } catch (e) {
    console.error("Fetch failed:", e);
  }
}

run().catch(console.error);
