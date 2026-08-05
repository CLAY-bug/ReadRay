// 一次性只读脚本：从 Codex app.asar 提取主题注册表与主题资源文件。
// 不新增依赖，使用 Node 原生 fs/buffer 解析 ASAR 格式。
// 只读取，不修改 Codex 安装目录。运行后产生的提取文件写入项目临时目录。
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";

const ASAR = process.argv[2];
const OUT = process.argv[3] || "scripts/codex-theme-extract";
const FILE_MATCH = process.argv[4] || ""; // 可选：文件名子串过滤

if (!ASAR || !existsSync(ASAR)) {
  console.error("usage: node extract.mjs <app.asar> <outdir> [filenameFilter]");
  process.exit(1);
}

const buf = readFileSync(ASAR);
// ASAR 头（Electron asar 格式）：
//   字节 0-7：外层 pickle（值 4 + payload 长度 4）
//   字节 8-15：内层 pickle（值 4 + headerSize，小端 uint32）
//   header JSON 从字节 16 开始；文件数据区从 16 + headerSize 对齐到 4 字节
if (buf.length < 16) throw new Error("too small to be asar");
const headerSize = buf.readUInt32LE(12);
const header = JSON.parse(buf.subarray(16, 16 + headerSize).toString("utf8"));

// 收集文件（相对路径 -> 目录树里的 offset+size）
const files = [];
function walk(node, prefix) {
  for (const [name, child] of Object.entries(node.files || {})) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (child.files) {
      walk(child, path);
    } else {
      files.push({ path, offset: Number(child.offset), size: child.size });
    }
  }
}
walk(header);

// 文件数据区从 header 结尾后对齐到 4 字节开始
let base = 16 + headerSize;
if (base % 4 !== 0) base += 4 - (base % 4);

let matched = 0;
const skipped = [];
for (const file of files) {
  if (FILE_MATCH && !file.path.includes(FILE_MATCH)) continue;
  const data = buf.subarray(base + file.offset, base + file.offset + file.size);
  const outPath = join(OUT, file.path);
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, data);
  matched += 1;
  console.log(`extracted ${file.path} (${file.size} bytes)`);
}
for (const file of files) {
  if (!FILE_MATCH || file.path.includes(FILE_MATCH)) continue;
  skipped.push(file.path);
}
if (skipped.length) console.log(`skipped ${skipped.length} files (not matching filter)`);
console.log(`parsed header with ${files.length} files, extracted ${matched}`);
