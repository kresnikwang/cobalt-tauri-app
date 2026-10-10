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
        // aes-256-cbc is unauthenticated: a wrong key/IV or tampered bytes is
        // rejected through PKCS#7 padding only ~255/256 of the time, so asserting
        // `.toThrow()` was flaky. The real guarantee is confidentiality — the
        // original plaintext must never be recovered. When padding happens to be
        // valid the output is garbled (and the caller's JSON.parse in
        // stream/manage.js rejects it), so assert that property deterministically.
        const original = Buffer.from(JSON.stringify({ x: 1 }));
        const assertUnreadable = (ciphertext, useIV, useSecret) => {
            try {
                const output = decryptStream(ciphertext, useIV, useSecret);
                expect(Buffer.compare(output, original)).not.toBe(0);
            } catch {
                // Invalid PKCS#7 padding: the decipher rejected the data, as expected.
            }
        };

        it('should fail to decrypt with wrong IV', () => {
            const wrongIV = randomBytes(16).toString('base64url');
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            assertUnreadable(ciphertext, wrongIV, secret);
        });

        it('should fail to decrypt with wrong secret', () => {
            const wrongSecret = randomBytes(32).toString('base64url');
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            assertUnreadable(ciphertext, iv, wrongSecret);
        });

        it('should fail to decrypt corrupt data', () => {
            const ciphertext = encryptStream({ x: 1 }, iv, secret);
            // Flip the last byte so the final (padding) block is affected.
            ciphertext[ciphertext.length - 1] ^= 0xff;
            assertUnreadable(ciphertext, iv, secret);
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
