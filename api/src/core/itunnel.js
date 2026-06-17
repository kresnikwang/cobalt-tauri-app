import stream from "../stream/stream.js";
import { getInternalTunnel } from "../stream/manage.js";
import { setTunnelPort } from "../config.js";
import { Green } from "../misc/console-text.js";
import express from "express";

const validateTunnel = (req, res) => {
    const ip = req.ip || req.socket.remoteAddress || '';
    const isLocal = ip === '127.0.0.1' || ip === '::1' || ip === '::ffff:127.0.0.1' || ip.endsWith('127.0.0.1');
    if (!isLocal) {
        console.warn(`[Tunnel Security] Rejected connection from non-loopback IP: ${ip}`);
        res.sendStatus(403);
        return;
    }

    if (String(req.query.id).length !== 21) {
        res.sendStatus(400);
        return;
    }

    const streamInfo = getInternalTunnel(req.query.id);
    if (!streamInfo) {
        res.sendStatus(404);
        return;
    }

    return streamInfo;
}

const streamTunnel = (req, res) => {
    const streamInfo = validateTunnel(req, res);
    if (!streamInfo) {
        return;
    }

    streamInfo.headers = new Map([
        ...Object.entries(req.headers),
        ...(streamInfo.headers || [])
    ]);

    return stream(res, { type: 'internal', data: streamInfo });
}

export const setupTunnelHandler = () => {
    const tunnelHandler = express();

    tunnelHandler.get('/itunnel', streamTunnel);

    // fallback
    tunnelHandler.use((_, res) => res.sendStatus(400));
    // error handler
    tunnelHandler.use((_, __, res, ____) => res.socket.end());


    const server = tunnelHandler.listen({
        port: 0,
        host: '127.0.0.1',
        exclusive: true
    }, () => {
        const { port } = server.address();
        console.log(`${Green('[✓]')} internal tunnel handler running on 127.0.0.1:${port}`);
        setTunnelPort(port);
    });

    server.on('error', (err) => {
        console.error('Tunnel server error:', err);
    });
}
