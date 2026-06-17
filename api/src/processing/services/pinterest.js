import { genericUserAgent } from "../../config.js";
import { resolveRedirectingURL } from "../url.js";

const videoRegex = /"url":"(https:\/\/v1\.pinimg\.com\/videos\/.*?)"/g;
const imageRegex = /src="(https:\/\/i\.pinimg\.com\/.*\.(jpg|gif))"/g;
const notFoundRegex = /"__typename"\s*:\s*"PinNotFound"/;

function cleanEscapedURL(url) {
    return url?.replaceAll('\\u002F', '/')
               .replaceAll('\\/', '/');
}

function videoScore(url) {
    const resolution = url.match(/\/(\d+)p\//)?.[1];
    if (resolution) return Number(resolution);

    const size = url.match(/\/(\d+)x(\d+)\//);
    if (size) return Number(size[2]);

    return url.endsWith('.mp4') ? 1 : 0;
}

function pickBestVideo(links) {
    const cleanLinks = [...new Set(links.map(cleanEscapedURL).filter(Boolean))];
    const mp4Links = cleanLinks.filter(link => link.endsWith('.mp4'));
    if (mp4Links.length) {
        return mp4Links.reduce((a, b) => videoScore(a) > videoScore(b) ? a : b);
    }

    return cleanLinks.find(link => link.endsWith('.m3u8'));
}

export default async function(o) {
    let id = o.id;

    if (!o.id && o.shortLink) {
        const patternMatch = await resolveRedirectingURL(`https://api.pinterest.com/url_shortener/${o.shortLink}/redirect/`);
        id = patternMatch?.id;
    }

    if (id.includes("--")) id = id.split("--")[1];
    if (!id) return { error: "fetch.fail" };

    const html = await fetch(`https://www.pinterest.com/pin/${id}/`, {
        headers: { "user-agent": genericUserAgent }
    }).then(r => r.text()).catch(() => {});

    const invalidPin = html.match(notFoundRegex);

    if (invalidPin) return { error: "fetch.empty" };

    if (!html) return { error: "fetch.fail" };

    const videoLink = pickBestVideo(
        [...html.matchAll(videoRegex)].map(([, link]) => link)
    );

    if (videoLink) return {
        urls: videoLink,
        filename: `pinterest_${id}.mp4`,
        audioFilename: `pinterest_${id}_audio`,
        isHLS: videoLink.endsWith('.m3u8'),
    }

    const imageLink = [...html.matchAll(imageRegex)]
                    .map(([, link]) => cleanEscapedURL(link))
                    .find(a => a.endsWith('.jpg') || a.endsWith('.gif'));

    if (!imageLink) return { error: "fetch.empty" };

    const imageType = imageLink.endsWith(".gif") ? "gif" : "jpg"

    if (imageLink) return {
        urls: imageLink,
        isPhoto: true,
        filename: `pinterest_${id}.${imageType}`
    }

    return { error: "fetch.empty" };
}
