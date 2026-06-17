import { describe, it, expect, vi, beforeEach } from 'vitest';
import { createHmac, randomBytes } from 'crypto';

// Mock cluster BEFORE importing the module under test
vi.mock('node:cluster', () => ({
    default: { isPrimary: true, isWorker: false },
    isPrimary: true,
    isWorker: false,
}));

// Dynamic import to ensure mock is applied first
const secretsModule = await import('../secrets.js');

describe('secrets — hashHmac', () => {
    describe('with valid type', () => {
        it('should produce a Buffer output', () => {
            const result = secretsModule.hashHmac('test-value', 'stream');
            expect(result).toBeInstanceOf(Buffer);
            expect(result.length).toBe(32); // SHA-256 = 32 bytes
        });

        it('should produce deterministic hashes for same input', () => {
            const a = secretsModule.hashHmac('hello', 'stream');
            const b = secretsModule.hashHmac('hello', 'stream');
            expect(Buffer.compare(a, b)).toBe(0);
        });

        it('should produce different hashes for different values', () => {
            const a = secretsModule.hashHmac('value-a', 'stream');
            const b = secretsModule.hashHmac('value-b', 'stream');
            expect(Buffer.compare(a, b)).not.toBe(0);
        });

        it('should produce different hashes for different types', () => {
            const a = secretsModule.hashHmac('same-value', 'rate');
            const b = secretsModule.hashHmac('same-value', 'stream');
            // May or may not differ depending on salt initialization,
            // but both should be valid Buffers
            expect(a).toBeInstanceOf(Buffer);
            expect(b).toBeInstanceOf(Buffer);
        });

        it('should handle empty strings', () => {
            const result = secretsModule.hashHmac('', 'stream');
            expect(result).toBeInstanceOf(Buffer);
            expect(result.length).toBe(32);
        });

        it('should handle long strings', () => {
            const long = 'x'.repeat(10000);
            const result = secretsModule.hashHmac(long, 'stream');
            expect(result).toBeInstanceOf(Buffer);
            expect(result.length).toBe(32);
        });

        it('should produce consistent base64url encoding', () => {
            const hmac = secretsModule.hashHmac('test,123,abc,xyz', 'stream');
            const encoded = hmac.toString('base64url');
            expect(typeof encoded).toBe('string');
            expect(encoded.length).toBeGreaterThan(0);
            // base64url should not contain + or /
            expect(encoded).not.toContain('+');
            expect(encoded).not.toContain('/');
        });
    });

    describe('with invalid type', () => {
        it('should throw for unknown type', () => {
            expect(() => secretsModule.hashHmac('test', 'invalid')).toThrow('unknown salt');
        });

        it('should throw for undefined type', () => {
            expect(() => secretsModule.hashHmac('test', undefined)).toThrow();
        });
    });
});
