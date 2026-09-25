// 巢湖学院泮山苑技术标 DOCX 审计脚本 (Node.js 版)
// 用法: node audit_docx.js
const fs = require('fs');
const path = require('path');

const TMP = process.env.TEMP;
const docXmlPath = path.join(TMP, 'docx_extract', 'word', 'document.xml');
const stylesXmlPath = path.join(TMP, 'docx_extract', 'word', 'styles.xml');

if (!fs.existsSync(docXmlPath)) {
  console.error('ERROR: document.xml 未找到，请先解压 docx \u2192 ' + docXmlPath);
  process.exit(1);
}

let doc = fs.readFileSync(docXmlPath, 'utf8');
let styles = fs.readFileSync(stylesXmlPath, 'utf8');

// ---------- 1. 基本统计 ----------
console.log('='.repeat(60));
console.log('DOCX 审计报告 (Node.js)');
console.log('='.repeat(60));

// 图片数量 (wp:inline / wp:anchor)
const imgCount = (doc.match(/<w:drawing>/g) || []).length;
console.log(`1. 图片数量: ${imgCount}`);
const relpath = path.join(TMP, 'docx_extract', 'word', '_rels', 'document.xml.rels');
if (fs.existsSync(relpath)) {
  const rels = fs.readFileSync(relpath, 'utf8');
  const imgs = (rels.match(/Target="media\//g) || []).length;
  console.log(`   media图片数: ${imgs}`);
}

// 段落数
const paraCount = (doc.match(/<w:p[ >]/g) || []).length;
// 表格
const maxTblId = doc.match(/<w:tblPr>/g);
const tblCount = (doc.match(/<w:tbl>/g) || []).length;
console.log(`2. 段落数: ${paraCount}, 表格数: ${tblCount}`);

// 文本提取
function extractText(xml) {
  const texts = [];
  const re = /<w:t[^>]*>([^<]*)<\/w:t>/g;
  let m;
  while ((m = re.exec(xml)) !== null) texts.push(m[1]);
  return texts.join('');
}
const allText = extractText(doc);
console.log(`3. 全文文本字符数(去标签后): ${allText.length}`);

// ---------- 2. 评分索引表 ----------
console.log('\n' + '='.repeat(60));
console.log('2. 评分索引与章节');
console.log('='.repeat(60));
const hasScoreIndex = allText.includes('评分索引');
console.log(`评分索引表: ${hasScoreIndex ? '存在 [OK]' : '不存在 [FAIL]'}`);
// 章节标题 (用 w:outlineLvl 或样式)
const headings = [];
const hRe = /<w:p\b[^>]*>([\s\S]*?)<\/w:p>/g;
let hm;
while ((hm = hRe.exec(doc)) !== null) {
  const paraXml = hm[0];
  if (!paraXml.includes('<w:outlineLvl')) continue;
  const lvlMatch = paraXml.match(/<w:outlineLvl w:val="(\d+)"/);
  if (!lvlMatch) continue;
  const lvl = parseInt(lvlMatch[1], 10);
  const text = extractText(paraXml);
  if (text.trim()) headings.push({ lvl, text: text.trim() });
}
console.log(`标题数(带大纲层级): ${headings.length}`);
headings.slice(0, 30).forEach(h => {
  console.log(`${'  '.repeat(h.lvl)}[L${h.lvl}] ${h.text.slice(0, 40)}`);
});

// ---------- 3. 关键数据一致性核对 ----------
console.log('\n' + '='.repeat(60));
console.log('3. 关键数据一致性核对 (DOCX实测 vs 清单/口径)');
console.log('='.repeat(60));

const checks = [
  { name: '灯具数量 应为3087 (清单口径,旧稿3880)', want: '3087', bad: '3880' },
  { name: '防护棚净高 应统一≥4.0m', want: '4.0m' },
  { name: '配电箱 682台', want: '682台' },
  { name: '外墙真石漆 14936㎡', want: '14936' },
  { name: '水性环氧 12231㎡', want: '12231' },
  { name: '插座 5301个', want: '5301' },
  { name: '双绞线 40174m', want: '40174' },
  { name: '大班器 98组', want: '98组' },
  { name: '工期 120日历天', want: '120日历天' },
  { name: '编号 2026AFAGZ50779', want: '2026AFAGZ50779' },
];

// 先统计 each want 出现次数
const results = {};
for (const c of checks) {
  let wantCount = (allText.match(new RegExp(esc(c.want), 'g')) || []).length;
  results[c.want] = wantCount;
  let note = wantCount >= 1 ? '[OK]' : '[FAIL]';
  console.log(`  ${c.name}: "${c.want}" 出现 ${wantCount} 次 ${note}`);
  if (c.bad) {
    const badCount = (allText.match(new RegExp(esc(c.bad), 'g')) || []).length;
    if (badCount > 0) console.log(`    ⚠️ 旧值 "${c.bad}" 仍出现 ${badCount} 次 [须删除]`);
  }
}

function esc(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

// ---------- 4. 防护棚净高一致性 ----------
console.log('\n' + '='.repeat(60));
console.log('4. 防护棚净高 2.5m vs 4.0m 矛盾检查');
console.log('='.repeat(60));
const n40 = (allText.match(/净高.{0,6}4\.0m/g) || []).length;
const n25 = (allText.match(/净高.{0,6}2\.5m/g) || []).length;
const n25road = (allText.match(/通行净高/g) || []).length;
console.log(`"净高...4.0m" 出现: ${n40} 次`);
console.log(`"净高...2.5m" 出现: ${n25} 次 ${n25 > 0 ? '[FAIL 矛盾]' : '[OK]'}`);
console.log(`"通行净高" 引用: ${n25road} 次`);

// ---------- 5. CPM/里程碑 ----------
console.log('\n' + '='.repeat(60));
console.log('5. 工期与CPM');
console.log('='.repeat(60));
console.log(`"115天" 出现: ${(allText.match(/115天/g) || []).length} 次`);
console.log(`"120日历天" 出现: ${(allText.match(/120日历天/g) || []).length} 次`);
console.log(`"关键线路" 出现: ${(allText.match(/关键线路/g) || []).length} 次`);
console.log(`"机动" 出现: ${(allText.match(/机动/g) || []).length} 次`);

// ---------- 6. 品牌覆盖 ----------
console.log('\n' + '='.repeat(60));
console.log('6. 品牌覆盖');
console.log('='.repeat(60));
const brands = ['海螺','东方雨虹','伟星','九牧','公牛','正泰','立邦','东鹏','欧普','远东','凤铝','步阳','奥普','坚朗','海湾','西卡','龙牌','兔宝宝','法狮龙','美的','一舟'];
let found = 0;
for (const b of brands) {
  const n = (allText.match(new RegExp(b, 'g')) || []).length;
  if (n > 0) found++;
  else console.log(`  ✗ 缺 ${b}`);
}
console.log(`品牌覆盖: ${found}/${brands.length} ${found === brands.length ? '[OK 全覆盖]' : '[FAIL]'}`);

// ---------- 7. 占位符/套话 ----------
console.log('\n' + '='.repeat(60));
console.log('7. 占位符与套话');
console.log('='.repeat(60));
const ph = ['XXX','xxx','×××','待定','____'];
for (const p of ph) {
  const n = (allText.match(new RegExp(esc(p), 'g')) || []).length;
  if (n > 0) console.log(`  ⚠️ 占位符 "${p}": ${n} 次`);
}
const cliche = (allText.match(/为.{0,8}提供.{0,6}保障/g) || []).length;
console.log(`"为...提供...保障" 套话: ${cliche} 次 ${cliche > 3 ? '[FAIL 模板化]' : '[OK]'}`);

// ---------- 8. 格式检查 (styles.xml) ----------
console.log('\n' + '='.repeat(60));
console.log('8. 样式检查 (styles.xml)');
console.log('='.repeat(60));
// Normal 样式
const normalMatch = styles.match(/<w:style [^>]*w:styleId="(?:a|Normal)"[^>]*>[\s\S]*?<\/w:style>/);
if (normalMatch) {
  const sXml = normalMatch[0];
  const fontMatch = sXml.match(/<w:rFonts[^>]*w:eastAsia="([^"]*)"/);
  const sizeMatch = sXml.match(/<w:sz w:val="(\d+)"/);
  const lineMatch = sXml.match(/<w:spacing[^>]*w:line="(\d+)"[^>]*w:lineRule="(\w+)"/);
  console.log(`Normal字体(eastAsia): ${fontMatch ? fontMatch[1] : '未设'}`);
  console.log(`Normal字号(sz/2磅): ${sizeMatch ? sizeMatch[1] / 2 + 'pt' : '未设'}`);
  console.log(`Normal行距: ${lineMatch ? lineMatch[1] / 20 + 'pt ' + lineMatch[2] : '未设'}`);
}

// 缺少 w:spacing 的异常 (如有多行距) - 简化: 检查是否有固定行距
const fixedLine = (styles.match(/w:lineRule="exact"/g) || []).length;
const autoLine = (styles.match(/w:lineRule="auto"/g) || []).length;
const atLeast = (styles.match(/w:lineRule="atLeast"/g) || []).length;
console.log(`样式行距规则: exact=${fixedLine}, auto=${autoLine}, atLeast=${atLeast}`);

// ---------- 9. 页边距 (sectPr) ----------
console.log('\n' + '='.repeat(60));
console.log('9. 页边距');
console.log('='.repeat(60));
const pgMar = doc.match(/<w:pgMar[^>]*>/);
if (pgMar) {
  const g = (k) => (pgMar[0].match(new RegExp(`w:${k}="(\\d+)"`)) || [null, null])[1];
  const twips2cm = (t) => (t ? Math.round((t / 567) * 100) / 100 : null);
  console.log(`上(top): ${twips2cm(g('top'))}cm, 下(bottom): ${twips2cm(g('bottom'))}cm`);
  console.log(`左(left): ${twips2cm(g('left'))}cm, 右(right): ${twips2cm(g('right'))}cm`);
  console.log(`(要求: 上2.5cm, 其余2.0cm)`);
}

// 是否A4
const pgSz = doc.match(/<w:pgSz[^>]*>/);
if (pgSz) {
  const w = (pgSz[0].match(/w:w="(\d+)"/) || [null, null])[1];
  const h = (pgSz[0].match(/w:h="(\d+)"/) || [null, null])[1];
  const cm = (t) => Math.round((t / 567) * 100) / 100;
  console.log(`纸张: ${cm(w)}cm × ${cm(h)}cm ${Math.abs(cm(w) - 21) < 0.5 && Math.abs(cm(h) - 29.7) < 0.5 ? '[A4 OK]' : '[非A4]'}`);
}

console.log('\n' + '='.repeat(60));
console.log('审计完成');
console.log('='.repeat(60));