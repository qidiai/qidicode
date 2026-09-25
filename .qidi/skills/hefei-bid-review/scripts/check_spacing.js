const fs = require('fs');
const xml = fs.readFileSync(process.env.TEMP + '/docx_extract/word/document.xml', 'utf8');

const exact = (xml.match(/w:lineRule="exact"/g) || []).length;
const auto = (xml.match(/w:lineRule="auto"/g) || []).length;
const atLeast = (xml.match(/w:lineRule="atLeast"/g) || []).length;

console.log('段落级 w:lineRule:');
console.log('  exact:', exact);
console.log('  auto:', auto);
console.log('  atLeast:', atLeast);

// w:line 值分布
const lineVals = {};
const re = /w:line="(\d+)"/g;
let m;
while ((m = re.exec(xml)) !== null) {
  lineVals[m[1]] = (lineVals[m[1]] || 0) + 1;
}
console.log('w:line值分布:', JSON.stringify(lineVals));

// 首个spacing
const sm = xml.match(/<w:spacing[^>]*>/);
console.log('首个spacing:', sm ? sm[0] : '无');

// 统计有spacing但无lineRule的
const spacingNoRule = (xml.match(/<w:spacing[^>]*w:line="\d+"[^>]*>/g) || []).filter(s => !s.includes('lineRule')).length;
console.log('有line但无lineRule的spacing:', spacingNoRule);
