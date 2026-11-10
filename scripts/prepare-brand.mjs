import {readFileSync,writeFileSync,copyFileSync} from 'node:fs';
import {buildLogo} from './generate-logo.mjs';
writeFileSync(new URL('../assets/logo.svg',import.meta.url),buildLogo());
const source=readFileSync(new URL('../assets/logo.svg',import.meta.url),'utf8');
const mark=source.replace(/<svg\b[^>]*>/,'').replace(/<\/svg>\s*$/,'').replace(/\s*<title[^>]*>.*?<\/title>/s,'').trim();
// macOS icon grid: 824px tile centered in a 1024px canvas, 185px corner radius.
const icon=`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="background" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse">
      <stop stop-color="#2b6b5b"/><stop offset="1" stop-color="#123a34"/>
    </linearGradient>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#background)"/>
  <g transform="translate(186 186) scale(5.1)">
  ${mark}
  </g>
</svg>\n`;
writeFileSync(new URL('../assets/icon.svg',import.meta.url),icon);
copyFileSync(new URL('../assets/logo.svg',import.meta.url),new URL('../public/logo.svg',import.meta.url));
copyFileSync(new URL('../assets/icon.svg',import.meta.url),new URL('../public/icon.svg',import.meta.url));
