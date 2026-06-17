export interface ServicePlatform {
  name: string;
  domain: string;
  color: string;
  bg: string;
  keywords: string[];
}

export const platforms: ServicePlatform[] = [
  { name: 'YouTube', domain: 'youtube.com', color: '#ff0000', bg: 'rgba(255, 0, 0, 0.15)', keywords: ['youtube', 'youtu.be'] },
  { name: 'Bilibili', domain: 'bilibili.com', color: '#00aeec', bg: 'rgba(0, 174, 236, 0.15)', keywords: ['bilibili', 'bili'] },
  { name: 'Instagram', domain: 'instagram.com', color: '#e1306c', bg: 'rgba(225, 48, 108, 0.15)', keywords: ['instagram'] },
  { name: 'Twitter / X', domain: 'x.com', color: '#ffffff', bg: 'rgba(255, 255, 255, 0.08)', keywords: ['twitter', 'x.com'] },
  { name: 'SoundCloud', domain: 'soundcloud.com', color: '#ff5500', bg: 'rgba(255, 85, 0, 0.15)', keywords: ['soundcloud'] },
  { name: 'Pinterest', domain: 'pinterest.com', color: '#bd081c', bg: 'rgba(189, 8, 28, 0.15)', keywords: ['pinterest'] },
  { name: 'Snapchat', domain: 'snapchat.com', color: '#fffc00', bg: 'rgba(255, 252, 0, 0.1)', keywords: ['snapchat'] },
  { name: 'Twitch', domain: 'twitch.tv', color: '#9146ff', bg: 'rgba(145, 70, 255, 0.15)', keywords: ['twitch.tv'] },
  { name: 'Dailymotion', domain: 'dailymotion.com', color: '#0066dc', bg: 'rgba(0, 102, 220, 0.15)', keywords: ['dailymotion'] },
  { name: 'Streamable', domain: 'streamable.com', color: '#0f766e', bg: 'rgba(15, 118, 110, 0.15)', keywords: ['streamable.com'] }
];

export function getServiceInfo(url: string, unknownLabel: string = 'Web Media') {
  const lower = url.toLowerCase();
  for (const platform of platforms) {
    if (platform.keywords.some(kw => lower.includes(kw))) {
      return {
        name: platform.name,
        color: platform.color,
        bg: platform.bg
      };
    }
  }
  return {
    name: unknownLabel,
    color: '#6366f1',
    bg: 'rgba(99, 102, 241, 0.15)'
  };
}
