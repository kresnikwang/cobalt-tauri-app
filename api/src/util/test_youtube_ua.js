import { Innertube } from "youtubei.js";

async function test() {
    console.log("Initializing Innertube...");
    const yt = await Innertube.create();
    console.log("Session client userAgent:", yt.session.context.client.userAgent);
}

test();
