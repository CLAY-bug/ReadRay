// 一次性数据生成脚本：从 hermitdave/FrequencyWords（MIT 授权，OpenSubtitles
// 语料）的 en_50k.txt 提取随包嵌入的纯英文词表。
//
// 用法：node scripts/gen-vocabulary.mjs <en_50k.txt 路径>
// 输出：src-tauri/src/vocabulary_data.txt（每行一词，按真实使用频率降序）
//
// 过滤规则：只保留纯 ASCII 拉丁词形（a-z，允许撇号和连字符），统一小写并去
// 重；运行时补全匹配大小写不敏感。原始频率数字不进入产物，顺序即频率序。
// 数据来源与授权需在 README 资产说明中保持归属。

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const sourcePath = process.argv[2];
const outputPath = resolve(scriptDir, "../src-tauri/src/vocabulary_data.txt");

if (!sourcePath) {
  console.error("用法：node scripts/gen-vocabulary.mjs <en_50k.txt 路径>");
  process.exit(1);
}

const raw = readFileSync(sourcePath, "utf8");
const seen = new Set();
const terms = [];

for (const line of raw.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed) {
    continue;
  }
  const word = trimmed.split(/\s+/)[0].toLowerCase();
  if (!/^[a-z]+(?:['-][a-z]+)*$/.test(word)) {
    continue;
  }
  if (seen.has(word)) {
    continue;
  }
  seen.add(word);
  terms.push(word);
}

writeFileSync(outputPath, `${terms.join("\n")}\n`, "utf8");
console.log(`已生成 ${terms.length} 个词形 -> ${outputPath}`);
