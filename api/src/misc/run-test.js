import { normalizeRequest } from "../processing/request.js";
import match from "../processing/match.js";
import { extract } from "../processing/url.js";

export async function runTest(url, params, expect) {
    const { success, data: normalized } = await normalizeRequest({ url, ...params });
    if (!success) {
        throw "invalid request";
    }

    const parsed = extract(normalized.url);
    if (parsed === null) {
        throw `invalid url: ${normalized.url}`;
    }

    const result = await match({
        host: parsed.host,
        patternMatch: parsed.patternMatch,
        params: normalized,
    });

    let error = [];
    if (expect.status !== result.body.status) {
        const detail = `${expect.status} (expected) != ${result.body.status} (actual)`;
        error.push(`status mismatch: ${detail}`);

        if (result.body.status === 'error') {
            error.push(`error code: ${result.body?.error?.code}`);
        }
    }

    if (expect.errorCode && expect.errorCode !== result.body?.error?.code) {
        const detail = `${expect.errorCode} (expected) != ${result.body.error.code} (actual)`
        error.push(`error mismatch: ${detail}`);
    }

    if (expect.code !== result.status) {
        const detail = `${expect.code} (expected) != ${result.status} (actual)`;
        error.push(`status code mismatch: ${detail}`);
    }

    if (error.length) {
        if (result.body.text) {
            error.push(`error message: ${result.body.text}`);
        }

        throw error.join('\n');
    }

    if (result.body.status === 'tunnel') {
        try {
            const tunnelUrl = new URL(result.body.url);
            const requiredParams = ['id', 'exp', 'sig', 'sec', 'iv'];
            const missing = requiredParams.filter(p => !tunnelUrl.searchParams.has(p));
            if (missing.length) {
                error.push(`tunnel URL missing params: ${missing.join(', ')}`);
            }

            // verify the stream data was stored by checking it can be retrieved
            const { verifyStream } = await import('../stream/manage.js');
            const id = tunnelUrl.searchParams.get('id');
            const hmac = tunnelUrl.searchParams.get('sig');
            const exp = tunnelUrl.searchParams.get('exp');
            const secret = tunnelUrl.searchParams.get('sec');
            const iv = tunnelUrl.searchParams.get('iv');

            if (id && hmac && exp && secret && iv) {
                const streamResult = await verifyStream(id, hmac, exp, secret, iv);
                if (!streamResult || streamResult.status) {
                    error.push(`tunnel verification failed: status ${streamResult?.status || 'unknown'}`);
                }
            }
        } catch (e) {
            error.push(`tunnel validation error: ${e.message || e}`);
        }
    }
}
