// 차등 시험 재료 생성기(개발용, 배포 묶음에 들어가지 않는다).
// 사용: 임시 폴더에서 `npm i quill-delta@5` 후 `node tools/gen-quill-fixtures.mjs > tests/fixtures/quill.json` (NODE_PATH로 그 node_modules를 가리킨다)
// 글 길이는 ASCII만 써서 JS(UTF-16)와 이 크레이트(UTF-8 바이트)가 같게 한다. embed는 길이 규칙이 달라 제외한다.
import { createRequire } from 'module';
const require = createRequire(process.env.QD_DIR ? process.env.QD_DIR + '/' : import.meta.url);
const Delta = require('quill-delta');
let s = 12345;
const rnd = (n) => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s % n; };
const attrs = (allowNull) => {
  if (rnd(2)) return undefined;
  const a = {};
  for (const k of ['b', 'c']) {
    const r = rnd(4);
    if (r === 0) a[k] = rnd(3);
    else if (r === 1 && allowNull) a[k] = null;
  }
  return Object.keys(a).length ? a : undefined;
};
const text = () => 'abcdef'.slice(0, 1 + rnd(4));
const doc = () => { let d = new Delta(); for (let i = 0, n = 1 + rnd(4); i < n; i++) d = d.insert(text(), attrs(false)); return d; };
const change = (len) => {
  let d = new Delta(); let left = len;
  for (let i = 0, n = 1 + rnd(4); i < n; i++) {
    const k = rnd(3);
    if (k === 0 || left === 0) d = d.insert(text(), attrs(false));
    else { const m = 1 + rnd(left); left -= m; d = k === 1 ? d.retain(m, attrs(true)) : d.delete(m); }
  }
  return d.chop();
};
const j = (d) => ({ ops: d.ops });
const cases = [];
for (let i = 0; i < 800; i++) {
  const base = doc(); const len = base.length();
  const a = change(len), b = change(len);
  cases.push({
    base: j(base), a: j(a), b: j(b),
    compose_base_a: j(base.compose(a)),
    t_b_after_a_prio: j(a.transform(b, true)),
    t_b_after_a_noprio: j(a.transform(b, false)),
    invert_a: j(a.invert(base)),
  });
}
console.log(JSON.stringify(cases));
