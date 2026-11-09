import {readFileSync,writeFileSync,copyFileSync} from 'node:fs';
const source=readFileSync(new URL('../assets/logo.svg',import.meta.url),'utf8');
const mark=source.replace(/<svg\b[^>]*>/,'').replace(/<\/svg>\s*$/,'').replace(/\s*<title[^>]*>.*?<\/title>/s,'');
const icon=`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="background" x1="180" y1="100" x2="840" y2="960" gradientUnits="userSpaceOnUse">
      <stop stop-color="#315d51"/><stop offset="0.55" stop-color="#183c37"/><stop offset="1" stop-color="#102c2c"/>
    </linearGradient>
    <filter id="shadow" x="0" y="0" width="1024" height="1024" filterUnits="userSpaceOnUse"><feDropShadow dx="0" dy="14" stdDeviation="14" flood-color="#061a18" flood-opacity="0.28"/></filter>
  </defs>
  <rect x="80" y="80" width="864" height="864" rx="192" fill="url(#background)" filter="url(#shadow)"/>
  <rect x="84" y="84" width="856" height="856" rx="188" fill="none" stroke="#c0e6cc" stroke-opacity="0.16" stroke-width="3"/>
  <g transform="translate(36 16) scale(7.4)">${mark}</g>
</svg>\n`;
writeFileSync(new URL('../assets/icon.svg',import.meta.url),icon);
copyFileSync(new URL('../assets/logo.svg',import.meta.url),new URL('../public/logo.svg',import.meta.url));
copyFileSync(new URL('../assets/icon.svg',import.meta.url),new URL('../public/icon.svg',import.meta.url));
