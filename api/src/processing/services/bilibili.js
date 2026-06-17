import { env } from "../../config.js";
import { resolveRedirectingURL } from "../url.js";
import { getCookie } from "../cookie/manager.js";

// TO-DO: higher quality downloads (currently requires an account)

const comHeaders = {
    "user-agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
    "referer": "https://www.bilibili.com",
    "origin": "https://www.bilibili.com",
    "accept": "application/json, text/plain, */*"
};

function getComHeaders(videoUrl) {
    const cookie = getCookie('bilibili')?.toString();
    return {
        ...comHeaders,
        referer: videoUrl || comHeaders.referer,
        ...(cookie ? { cookie } : {})
    };
}

function getBest(content, maxQualityId = Infinity) {
    return content?.filter(v => v.baseUrl || v.url)
                .filter(v => !v.id || v.id <= maxQualityId)
                .map(v => (v.baseUrl = v.baseUrl || v.url, v))
                .reduce((a, b) => a?.bandwidth > b?.bandwidth ? a : b);
}

function extractBestQuality(dashData, maxQualityId) {
    const bestVideo = getBest(dashData.video, maxQualityId),
          bestAudio = getBest(dashData.audio);

    if (!bestVideo || !bestAudio) return [];
    return [ bestVideo, bestAudio ];
}

function getQualityId(quality) {
    const requested = quality === "max" ? 120 : Number(quality);

    if (requested >= 1080) return 80;
    if (requested >= 720) return 64;
    if (requested >= 480) return 32;
    return 16;
}

async function com_download(id, partId, quality) {
    try {
        const headers = getComHeaders(`https://www.bilibili.com/video/${id}/`);
        const metadataUrl = `https://api.bilibili.com/x/web-interface/view?bvid=${id}`;
        const metadataRes = await fetch(metadataUrl, {
            headers
        });
        if (!metadataRes.ok) {
            return { error: "fetch.fail" };
        }
        
        const metadataJson = await metadataRes.json();
        if (metadataJson.code !== 0 || !metadataJson.data) {
            return { error: "fetch.empty" };
        }
        
        const aid = metadataJson.data.aid;
        const pages = metadataJson.data.pages || [];
        if (pages.length === 0) {
            return { error: "fetch.empty" };
        }
        
        // partId is 1-indexed (e.g. p=1 is the first part)
        const pIdx = partId ? Number(partId) - 1 : 0;
        if (pIdx < 0 || pIdx >= pages.length) {
            return { error: "fetch.empty" };
        }
        const cid = pages[pIdx].cid;
        
        const qualityId = getQualityId(quality);
        const playurl = `https://api.bilibili.com/x/player/playurl?avid=${aid}&cid=${cid}&qn=${qualityId}&type=&otype=json&fourk=1&fnver=0&fnval=4048`;
        const playRes = await fetch(playurl, {
            headers
        });
        if (!playRes.ok) {
            return { error: "fetch.fail" };
        }
        
        const playJson = await playRes.json();
        if (playJson.code !== 0 || !playJson.data) {
            return { error: "fetch.empty" };
        }
        
        const dashData = playJson.data.dash;
        if (!dashData) {
            return { error: "fetch.empty" };
        }
        
        if (playJson.data.timelength > env.durationLimit * 1000) {
            return { error: "content.too_long" };
        }
        
        const [ video, audio ] = extractBestQuality(dashData, qualityId);
        if (!video || !audio) {
            return { error: "fetch.empty" };
        }
        
        let filenameBase = `bilibili_${id}`;
        if (partId) {
            filenameBase += `_${partId}`;
        }
        
        return {
            urls: [video.baseUrl, audio.baseUrl],
            headers,
            audioFilename: `${filenameBase}_audio`,
            filename: `${filenameBase}_${video.width}x${video.height}.mp4`,
        };
    } catch {
        return { error: "fetch.fail" };
    }
}

async function tv_download(id) {
    const url = new URL(
        'https://api.bilibili.tv/intl/gateway/web/playurl'
        + '?s_locale=en_US&platform=web&qn=64&type=0&device=wap'
        + '&tf=0&spm_id=bstar-web.ugc-video-detail.0.0&from_spm_id='
    );

    url.searchParams.set('aid', id);

    const { data } = await fetch(url).then(a => a.json());
    if (!data?.playurl?.video) {
        return { error: "fetch.empty" };
    }

    const [ video, audio ] = extractBestQuality({
        video: data.playurl.video.map(s => s.video_resource)
                                 .filter(s => s.codecs.includes('avc1')),
        audio: data.playurl.audio_resource
    });

    if (!video || !audio) {
        return { error: "fetch.empty" };
    }

    if (video.duration > env.durationLimit * 1000) {
        return { error: "content.too_long" };
    }

    return {
        urls: [video.url, audio.url],
        audioFilename: `bilibili_tv_${id}_audio`,
        filename: `bilibili_tv_${id}.mp4`
    };
}

export default async function({ comId, tvId, comShortLink, partId, quality }) {
    if (comShortLink) {
        const patternMatch = await resolveRedirectingURL(`https://b23.tv/${comShortLink}`);
        comId = patternMatch?.comId;
    }

    if (comId) {
        return com_download(comId, partId, quality);
    } else if (tvId) {
        return tv_download(tvId);
    }

    return { error: "fetch.fail" };
}
