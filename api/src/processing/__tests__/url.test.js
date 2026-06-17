import { describe, it, expect } from 'vitest';
import { normalizeURL } from '../url.js';

describe('url — normalizeURL', () => {
    describe('YouTube URLs', () => {
        it('should keep standard watch URL', () => {
            const url = normalizeURL('https://www.youtube.com/watch?v=dQw4w9WgXcQ');
            expect(url.href).toBe('https://www.youtube.com/watch?v=dQw4w9WgXcQ');
        });

        it('should convert youtu.be short link', () => {
            const url = normalizeURL('https://youtu.be/dQw4w9WgXcQ');
            expect(url.hostname).toBe('youtube.com');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });

        it('should convert /live/ URL to /watch', () => {
            const url = normalizeURL('https://www.youtube.com/live/dQw4w9WgXcQ');
            expect(url.pathname).toBe('/watch');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });

        it('should convert /shorts/ URL to /watch', () => {
            const url = normalizeURL('https://www.youtube.com/shorts/dQw4w9WgXcQ');
            expect(url.pathname).toBe('/watch');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });

        it('should strip query params except v', () => {
            const url = normalizeURL('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=30&feature=share');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
            expect(url.searchParams.get('t')).toBeNull();
        });
    });

    describe('Twitter/X URLs', () => {
        it('should keep twitter.com link', () => {
            const url = normalizeURL('https://twitter.com/user/status/123');
            expect(url.hostname).toBe('twitter.com');
            expect(url.pathname).toBe('/user/status/123');
        });

        it('should handle x.com links', () => {
            const url = normalizeURL('https://x.com/user/status/123');
            expect(url.hostname).toBe('twitter.com');
        });

        it('should handle vxtwitter.com links', () => {
            const url = normalizeURL('https://vxtwitter.com/user/status/123');
            expect(url.hostname).toBe('twitter.com');
        });
    });

    describe('TikTok URLs', () => {
        it('should keep standard tiktok URL', () => {
            const url = normalizeURL('https://www.tiktok.com/@user/video/123456789');
            expect(url.hostname).toBe('www.tiktok.com');
            expect(url.pathname).toBe('/@user/video/123456789');
        });
    });

    describe('Instagram URLs', () => {
        it('should handle ddinstagram embeds', () => {
            const url = normalizeURL('https://ddinstagram.com/p/abc123');
            expect(url.hostname).toBe('instagram.com');
        });
    });

    describe('Bilibili URLs', () => {
        it('should handle b23.tv short links', () => {
            const url = normalizeURL('https://b23.tv/abc123');
            expect(url.hostname).toBe('bilibili.com');
            expect(url.pathname).toBe('/_shortLink/abc123');
        });
    });

    describe('URL cleaning', () => {
        it('should remove trailing slashes', () => {
            const url = normalizeURL('https://www.youtube.com/watch?v=dQw4w9WgXcQ/');
            expect(url.pathname).not.to.include('//');
        });

        it('should remove hash fragments', () => {
            const url = normalizeURL('https://www.youtube.com/watch?v=dQw4w9WgXcQ#fragment');
            expect(url.hash).toBe('');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });

        it('should remove port numbers', () => {
            const url = normalizeURL('https://example.com:8080/path');
            expect(url.port).toBe('');
        });

        it('should strip tracking query params', () => {
            const url = normalizeURL('https://example.com/path?utm_source=test&ref=share');
            expect(url.search).toBe('');
        });
    });

    describe('Reddit URLs', () => {
        it('should convert v.redd.it to reddit.com/video', () => {
            const url = normalizeURL('https://v.redd.it/abc123def456');
            expect(url.hostname).toBe('www.reddit.com');
            expect(url.pathname).toBe('/video/abc123def456');
        });
    });

    describe('Twitch URLs', () => {
        it('should convert clips.twitch.tv', () => {
            const url = normalizeURL('https://clips.twitch.tv/abc123');
            // aliasURL transforms clips.twitch.tv → twitch.tv/_/clip/...
            expect(url.pathname).toBe('/_/clip/abc123');
        });
    });

    describe('Dailymotion URLs', () => {
        it('should handle dai.ly short links', () => {
            const url = normalizeURL('https://dai.ly/abc123');
            expect(url.hostname).toBe('dailymotion.com');
            expect(url.pathname).toBe('/video/abc123');
        });
    });

    describe('Edge cases', () => {
        it('should handle URL with newline characters', () => {
            const url = normalizeURL('https://www.youtube.com/watch?v=dQw4w9WgXcQ\n');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });

        it('should handle URLs with https// (missing colon)', () => {
            const url = normalizeURL('https//www.youtube.com/watch?v=dQw4w9WgXcQ');
            expect(url.searchParams.get('v')).toBe('dQw4w9WgXcQ');
        });
    });
});
