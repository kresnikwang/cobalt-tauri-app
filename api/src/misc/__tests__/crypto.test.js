import { describe, it, expect } from 'vitest';
import { randomBytes } from 'crypto';
import { encryptStream, decryptStream } from '../crypto.js';

describe('crypto — encryptStream / decryptStream', () => {
    const iv = randomBytes(16).toString('base64url');
    const secret = randomBytes(32).toString('base64url');

    describe('round-trip', () => {
        it('should encrypt and decrypt a simple object', () => {
            const plaintext = { hello: 'world', num: 42 };
            const ciphertext = encryptStream(plaintext, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual(plaintext);
        });

        it('should encrypt and decrypt stream metadata', () => {
            const streamData = {
                exp: Date.now() + 90000,
                type: 'proxy',
                urls: 'https://example.com/video.mp4',
                service: 'youtube',
                filename: 'My Video.mp4',
                requestIP: '192.0.2.1',
                headers: { 'User-Agent': 'test' },
                metadata: { title: 'Test' },
                audioBitrate: '128',
                audioCopy: true,
                audioFormat: 'mp3',
                isHLS: false,
                subtitles: 'https://example.com/sub.vtt',
            };
            const ciphertext = encryptStream(streamData, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual(streamData);
        });
    });

    describe('integrity', () => {
        it('should produce different ciphertext for different data', () => {
            const a = encryptStream({ x: 1 }, iv, secret);
            const b = encryptStream({ x: 2 }, iv, secret);
            expect(Buffer.compare(a, b)).not.toBe(0);
        });

        it('should produce different ciphertext with different IV', () => {
            const iv2 = randomBytes(16).toString('base64url');
            const a = encryptStream({ x: 1 }, iv, secret);
            const b = encryptStream({ x: 1 }, iv2, secret);
            expect(Buffer.compare(a, b)).not.toBe(0);
        });

        it('should produce different ciphertext with different secret', () => {
            const secret2 = randomBytes(32).toString('base64url');
            const a = encryptStream({ x: 1 }, iv, secret);
            const b = encryptStream({ x: 1 }, iv, secret2);
            expect(Buffer.compare(a, b)).not.toBe(0);
        });
    });

    describe('decryption errors', () => {
        it('should fail to decrypt with wrong IV', () => {
            const wrongIV = randomBytes(16).toString('base64url');
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            expect(() => decryptStream(ciphertext, wrongIV, secret)).toThrow();
        });

        it('should fail to decrypt with wrong secret', () => {
            const wrongSecret = randomBytes(32).toString('base64url');
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            expect(() => decryptStream(ciphertext, iv, wrongSecret)).toThrow();
        });

        it('should fail to decrypt corrupt data', () => {
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            // Tamper with the ciphertext
            ciphertext[0] = ciphertext[0] ^ 0xFF;
            expect(() => decryptStream(ciphertext, iv, secret)).toThrow();
        });
    });

    describe('edge cases', () => {
        it('should handle empty objects', () => {
            const ciphertext = encryptStream({}, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual({});
        });

        it('should handle nested objects', () => {
            const nested = { a: { b: { c: [1, 2, 3] } } };
            const ciphertext = encryptStream(nested, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual(nested);
        });

        it('should handle unicode strings', () => {
            const unicode = { name: 'テスト', emoji: '🎉🚀' };
            const ciphertext = encryptStream(unicode, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual(unicode);
        });

        it('should handle long data', () => {
            const longString = 'x'.repeat(10000);
            const data = { long: longString };
            const ciphertext = encryptStream(data, iv, secret);
            const decrypted = decryptStream(ciphertext, iv, secret);
            expect(JSON.parse(decrypted.toString())).toEqual(data);
        });
    });
});
