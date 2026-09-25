import { describe, expect, it } from 'vitest';

import { createAPIURL } from '../api-url.js';

describe('createAPIURL', () => {
    it('creates an endpoint below a root API URL', () => {
        expect(createAPIURL('https://api.example.com', '/tunnel').toString())
            .toBe('https://api.example.com/tunnel');
    });

    it('preserves a reverse-proxy path prefix', () => {
        expect(createAPIURL('http://api.example.com/cobalt-api', '/tunnel').toString())
            .toBe('http://api.example.com/cobalt-api/tunnel');
    });

    it('normalizes trailing and leading slashes', () => {
        expect(createAPIURL('http://api.example.com/cobalt-api/', '/yt-dlp').toString())
            .toBe('http://api.example.com/cobalt-api/yt-dlp');
    });
});
