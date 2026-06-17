import { existsSync }  from 'node:fs';
import { join, parse, dirname } from 'node:path';
import { cwd }         from 'node:process';
import { readFile }    from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const findFile = (file) => {
    // 1. Try resolving starting from current working directory
    try {
        let dir = cwd();
        while (dir && dir !== parse(dir).root) {
            if (existsSync(join(dir, file))) {
                return dir;
            }
            dir = join(dir, '../');
        }
    } catch {}

    // 2. Try resolving starting from the module's file location
    try {
        let dir = dirname(fileURLToPath(import.meta.url));
        while (dir && dir !== parse(dir).root) {
            if (existsSync(join(dir, file))) {
                return dir;
            }
            dir = join(dir, '../');
        }
    } catch {}
}

const root = findFile('.git');
const pack = findFile('package.json');

const readGit = async (filename) => {
    if (!root) {
        return '';
    }
    try {
        return await readFile(join(root, filename), 'utf8');
    } catch {
        return '';
    }
}

export const getCommit = async () => {
    try {
        const content = await readGit('.git/logs/HEAD');
        if (!content) return 'unknown';
        return content
                ?.split('\n')
                ?.filter(String)
                ?.pop()
                ?.split(' ')[1] || 'unknown';
    } catch {
        return 'unknown';
    }
}

export const getBranch = async () => {
    if (process.env.CF_PAGES_BRANCH) {
        return process.env.CF_PAGES_BRANCH;
    }

    if (process.env.WORKERS_CI_BRANCH) {
        return process.env.WORKERS_CI_BRANCH;
    }

    try {
        const content = await readGit('.git/HEAD');
        if (!content) return 'unknown';
        return content
                ?.replace(/^ref: refs\/heads\//, '')
                ?.trim() || 'unknown';
    } catch {
        return 'unknown';
    }
}

export const getRemote = async () => {
    try {
        const content = await readGit('.git/config');
        if (!content) return 'unknown';
        let remote = content
                        ?.split('\n')
                        ?.find(line => line.includes('url = '))
                        ?.split('url = ')[1];

        if (remote?.startsWith('git@')) {
            remote = remote.split(':')[1];
        } else if (remote?.startsWith('http')) {
            remote = new URL(remote).pathname.substring(1);
        }

        remote = remote?.replace(/\.git$/, '');
        return remote || 'unknown';
    } catch {
        return 'unknown';
    }
}

export const getVersion = async () => {
    if (!pack) {
        return '1.0.13';
    }

    try {
        const { version } = JSON.parse(
            await readFile(join(pack, 'package.json'), 'utf8')
        );
        return version || '1.0.13';
    } catch {
        return '1.0.13';
    }
}
