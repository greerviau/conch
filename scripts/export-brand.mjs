import {copyFileSync,readFileSync,writeFileSync} from 'node:fs';
copyFileSync(new URL('../.runtime/brand-icon/1024x1024.png',import.meta.url),new URL('../assets/app-icon.png',import.meta.url));
copyFileSync(new URL('../.runtime/brand-logo/512x512.png',import.meta.url),new URL('../assets/logo.png',import.meta.url));

const path=new URL('../src-tauri/icons/icon.icns',import.meta.url);
const icon=readFileSync(path);
if(icon.subarray(0,4).toString()!=='icns'||icon.readUInt32BE(4)!==icon.length)throw new Error('Invalid generated ICNS header');
const chunks=[];
for(let offset=8;offset<icon.length;){
 const length=icon.readUInt32BE(offset+4);
 if(length<8||offset+length>icon.length)throw new Error('Invalid generated ICNS chunk');
 chunks.push(icon.subarray(offset,offset+length));offset+=length;
}
chunks.sort((a,b)=>a.subarray(0,4).compare(b.subarray(0,4)));
writeFileSync(path,Buffer.concat([icon.subarray(0,8),...chunks]));
