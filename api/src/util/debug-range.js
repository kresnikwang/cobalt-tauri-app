import { request } from "undici";
import { getHeaders } from "../stream/shared.js";
import youtube from "../processing/services/youtube.js";

async function main() {
    const videoId = "boW53RZdeNg";
    try {
        const result = await youtube({
            id: videoId,
            quality: "1080",
            codec: "h264",
            isAudioOnly: false,
            isAudioMuted: false,
            youtubeHLS: false
        });
        
        const testUrl = [result.urls].flat()[0];
        const baseHeaders = getHeaders("youtube");
        
        const ranges = [
            "bytes=0-2000000",   // 2,000,000 bytes (failed in handleChunkedStream)
            "bytes=0-2097152",   // exactly 2MB (succeeded in task-311)
            "bytes=0-1048576",   // exactly 1MB (succeeded in task-305)
            "bytes=0-1000000"    // 1,000,000 bytes
        ];
        
        for (const r of ranges) {
            console.log(`\nTesting range: ${r}`);
            const res = await request(testUrl, {
                headers: {
                    ...baseHeaders,
                    Range: r
                },
                method: "GET",
                maxRedirections: 4
            });
            console.log("Status:", res.statusCode);
            await res.body.dump();
        }
    } catch (e) {
        console.error("Test failed:", e);
    }
}

main();
