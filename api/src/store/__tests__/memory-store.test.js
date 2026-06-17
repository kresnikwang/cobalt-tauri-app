import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import MemoryStore from '../memory-store.js';

// Use unique store names across test suites to avoid global _stores set collisions
let storeCounter = 0;
const uniqueName = (base) => `${base}-${++storeCounter}`;

describe('MemoryStore', () => {
    let store;

    beforeEach(() => {
        vi.useFakeTimers();
        store = new MemoryStore(uniqueName('test-store'));
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    describe('constructor', () => {
        it('should create a store with the given name', () => {
            expect(store.id).toBeTruthy();
            expect(store.id).toMatch(/^TEST-STORE/);
        });

        it('should throw if a store with the same name already exists', () => {
            const name = uniqueName('dup-store');
            new MemoryStore(name); // first instance
            expect(() => new MemoryStore(name)).toThrow('store already exists');
        });
    });

    describe('set / get', () => {
        it('should store and retrieve a string value', async () => {
            await store.set('key1', 'hello');
            const val = await store.get('key1');
            expect(val).toBe('hello');
        });

        it('should store and retrieve an object', async () => {
            const obj = { a: 1, b: [2, 3] };
            await store.set('key2', JSON.stringify(obj));
            const val = await store.get('key2');
            expect(JSON.parse(val)).toEqual(obj);
        });

        it('should return null for non-existent keys', async () => {
            const val = await store.get('nonexistent');
            expect(val).toBeNull();
        });

        it('should overwrite existing values', async () => {
            await store.set('key3', 'first');
            await store.set('key3', 'second');
            const val = await store.get('key3');
            expect(val).toBe('second');
        });

        it('should convert numeric keys to strings', async () => {
            await store.set(123, 'numeric-key');
            const val = await store.get('123');
            expect(val).toBe('numeric-key');
        });
    });

    describe('TTL expiration', () => {
        it('should return null for expired entries', async () => {
            await store.set('temp', 'expiring', 1); // 1 second TTL
            // MIN_THRESHOLD_MS = 2500, so sweep fires at max(2500, 1000) = 2500
            vi.advanceTimersByTime(3000);
            const val = await store.get('temp');
            expect(val).toBeNull();
        });

        it('should keep non-expired entries', async () => {
            await store.set('perm', 'permanent', 3600); // 1 hour TTL
            vi.advanceTimersByTime(1000); // 1 second later
            const val = await store.get('perm');
            expect(val).toBe('permanent');
        });

        it('should keep entries with no TTL indefinitely', async () => {
            await store.set('forever', 'eternal'); // No TTL
            vi.advanceTimersByTime(86400000); // 24 hours later
            const val = await store.get('forever');
            expect(val).toBe('eternal');
        });

        it('should update TTL when re-setting', async () => {
            await store.set('renew', 'old', 1);
            vi.advanceTimersByTime(500);
            await store.set('renew', 'new', 10);
            // Advance past original TTL window but within new one
            vi.advanceTimersByTime(3000);
            const val = await store.get('renew');
            expect(val).toBe('new');
        });
    });

    describe('has', () => {
        it('should return true for existing keys', async () => {
            await store.set('exists', 'yes');
            const has = await store.has('exists');
            expect(has).toBe(true);
        });

        it('should return false for non-existing keys', async () => {
            const has = await store.has('nope');
            expect(has).toBe(false);
        });
    });

    describe('concurrency', () => {
        it('should handle multiple concurrent writes', async () => {
            await Promise.all([
                store.set('a', '1'),
                store.set('b', '2'),
                store.set('c', '3'),
            ]);
            const [a, b, c] = await Promise.all([
                store.get('a'),
                store.get('b'),
                store.get('c'),
            ]);
            expect(a).toBe('1');
            expect(b).toBe('2');
            expect(c).toBe('3');
        });
    });
});
