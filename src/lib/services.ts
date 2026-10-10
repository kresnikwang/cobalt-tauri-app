export interface ServicePlatform {
  /** Stable identifier — used for i18n lookup and capability rules, never for display. */
  id: string;
  /** Fallback display name, used only if no translation exists for the active locale. */
  fallbackName: string;
  domain: string;
  color: string;
  bg: string;
  /** Host names (and subdomains) this platform owns. */
  hosts: string[];
  /** Last-resort substring match for non-http URLs such as capture://… */
  keywords: string[];
}

export const platforms: ServicePlatform[] = [
  { id: 'youtube', fallbackName: 'YouTube', domain: 'youtube.com', color: '#ff0000', bg: 'rgba(255, 0, 0, 0.15)', hosts: ['youtube.com', 'youtu.be'], keywords: ['youtube', 'youtu.be'] },
  { id: 'bilibili', fallbackName: 'Bilibili', domain: 'bilibili.com', color: '#00aeec', bg: 'rgba(0, 174, 236, 0.15)', hosts: ['bilibili.com', 'b23.tv'], keywords: ['bilibili', 'bili'] },
  { id: 'xinpianchang', fallbackName: '新片场', domain: 'xinpianchang.com', color: '#f5b21b', bg: 'rgba(245, 178, 27, 0.15)', hosts: ['xinpianchang.com'], keywords: ['xinpianchang'] },
  { id: 'instagram', fallbackName: 'Instagram', domain: 'instagram.com', color: '#e1306c', bg: 'rgba(225, 48, 108, 0.15)', hosts: ['instagram.com', 'ddinstagram.com'], keywords: ['instagram'] },
  { id: 'twitter', fallbackName: 'Twitter / X', domain: 'x.com', color: '#1d1d1f', bg: 'rgba(29, 29, 31, 0.08)', hosts: ['x.com', 'twitter.com', 'vxtwitter.com', 'fixvx.com'], keywords: ['twitter', 'x.com'] },
  { id: 'soundcloud', fallbackName: 'SoundCloud', domain: 'soundcloud.com', color: '#ff5500', bg: 'rgba(255, 85, 0, 0.15)', hosts: ['soundcloud.com'], keywords: ['soundcloud'] },
  { id: 'pinterest', fallbackName: 'Pinterest', domain: 'pinterest.com', color: '#bd081c', bg: 'rgba(189, 8, 28, 0.15)', hosts: ['pinterest.com'], keywords: ['pinterest'] },
  { id: 'dailymotion', fallbackName: 'Dailymotion', domain: 'dailymotion.com', color: '#0066dc', bg: 'rgba(0, 102, 220, 0.15)', hosts: ['dailymotion.com', 'dai.ly'], keywords: ['dailymotion'] }
];

/** Strips a leading "www." and lowercases, so host comparisons are stable. */
function normalizeHost(url: string): string {
  try {
    return new URL(url).hostname.toLowerCase().replace(/^www\./, '');
  } catch {
    return '';
  }
}

function matchPlatform(id: string): ServicePlatform | undefined {
  return platforms.find(platform => platform.id === id);
}

/**
 * Resolves a platform by matching the URL's host, not by scanning the whole string.
 * A substring scan would label "https://notyoutube.com.evil/watch" as YouTube.
 */
export function getServiceId(url: string): string | null {
  const host = normalizeHost(url);
  if (host) {
    const hit = platforms.find(platform =>
      platform.hosts.some(h => host === h || host.endsWith('.' + h))
    );
    if (hit) return hit.id;
  }
  // capture://… and other non-http schemes have no host; fall back to a keyword scan.
  const lower = url.toLowerCase();
  for (const platform of platforms) {
    if (platform.keywords.some(keyword => lower.includes(keyword))) return platform.id;
  }
  return null;
}

export function getServiceInfo(url: string, unknownLabel: string, nameFor: (id: string) => string) {
  const id = getServiceId(url);
  const platform = id ? matchPlatform(id) : undefined;
  if (!platform) {
    return { name: unknownLabel, color: '#2f2d29', bg: 'rgba(47, 45, 41, 0.1)' };
  }
  return { name: nameFor(platform.id), color: platform.color, bg: platform.bg };
}
