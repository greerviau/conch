import {copyFileSync} from 'node:fs';
copyFileSync(new URL('../.runtime/brand-icon/1024x1024.png',import.meta.url),new URL('../assets/app-icon.png',import.meta.url));
copyFileSync(new URL('../.runtime/brand-logo/512x512.png',import.meta.url),new URL('../assets/logo.png',import.meta.url));
