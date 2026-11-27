//! 두 문서(삽입만 있는 delta)의 차이를 변경 delta로 만든다: `a.compose(&a.diff(&b)) == b`.
//!
//! 앞뒤 공통부분을 걷어낸 가운데를 글자(원자) 단위 LCS로 맞춘다. 가운데가 너무 크면
//! (`LCS_LIMIT` 초과) 통째 교체로 물러난다. 속성만 다른 같은 글자는 retain+속성 변경이다.

use super::delta::{Attrs, Delta, Insert, Op};
use serde_json::Value;

/// 가운데 구간의 (앞 원자 수 x 뒤 원자 수) 상한.
const LCS_LIMIT: usize = 4_000_000;

#[derive(Clone, PartialEq)]
enum Kind {
    Ch(char),
    Embed(Value),
}

#[derive(Clone)]
struct Atom {
    kind: Kind,
    attrs: Option<Attrs>,
}

impl Atom {
    fn len(&self) -> usize {
        match &self.kind {
            Kind::Ch(c) => c.len_utf8(),
            Kind::Embed(_) => super::EMBED_LEN,
        }
    }
    fn same_content(&self, o: &Atom) -> bool {
        self.kind == o.kind
    }
}

fn atoms(d: &Delta) -> Vec<Atom> {
    let mut v = Vec::new();
    for op in d.ops() {
        if let Op::Insert { ins, attrs } = op {
            match ins {
                Insert::Text(s) => v.extend(s.chars().map(|c| Atom {
                    kind: Kind::Ch(c),
                    attrs: attrs.clone(),
                })),
                Insert::Embed(e) => v.push(Atom {
                    kind: Kind::Embed(e.clone()),
                    attrs: attrs.clone(),
                }),
            }
        }
    }
    v
}

/// 속성 `from`에서 `to`로 가는 retain 속성(없으면 None).
fn attr_change(from: &Option<Attrs>, to: &Option<Attrs>) -> Option<Attrs> {
    let mut out = Attrs::new();
    let empty = Attrs::new();
    let (f, t) = (
        from.as_ref().unwrap_or(&empty),
        to.as_ref().unwrap_or(&empty),
    );
    for (k, v) in t {
        if f.get(k) != Some(v) {
            out.insert(k.clone(), v.clone());
        }
    }
    for k in f.keys() {
        if !t.contains_key(k) {
            out.insert(k.clone(), Value::Null);
        }
    }
    (!out.is_empty()).then_some(out)
}

enum Step {
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

fn align(a: &[Atom], b: &[Atom]) -> Vec<Step> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre].same_content(&b[pre]) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre
        && suf < b.len() - pre
        && a[a.len() - 1 - suf].same_content(&b[b.len() - 1 - suf])
    {
        suf += 1;
    }
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut steps: Vec<Step> = (0..pre).map(|i| Step::Same(i, i)).collect();
    if am.len().saturating_mul(bm.len()) > LCS_LIMIT {
        steps.extend((0..am.len()).map(|i| Step::Del(pre + i)));
        steps.extend((0..bm.len()).map(|j| Step::Ins(pre + j)));
    } else {
        // lcs[i][j] = am[i..]와 bm[j..]의 최장 공통 길이.
        let (n, m) = (am.len(), bm.len());
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        let at = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[at(i, j)] = if am[i].same_content(&bm[j]) {
                    lcs[at(i + 1, j + 1)] + 1
                } else {
                    lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && am[i].same_content(&bm[j]) {
                steps.push(Step::Same(pre + i, pre + j));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[at(i + 1, j)] >= lcs[at(i, j + 1)]) {
                steps.push(Step::Del(pre + i));
                i += 1;
            } else {
                steps.push(Step::Ins(pre + j));
                j += 1;
            }
        }
    }
    let (la, lb) = (a.len(), b.len());
    steps.extend((0..suf).map(|k| Step::Same(la - suf + k, lb - suf + k)));
    steps
}

impl Delta {
    /// `self`(문서)를 `target`(문서)으로 바꾸는 변경 delta.
    pub fn diff(&self, target: &Delta) -> Delta {
        let (a, b) = (atoms(self), atoms(target));
        let mut out = Delta::new();
        for s in align(&a, &b) {
            match s {
                Step::Same(i, j) => out.push(Op::Retain {
                    n: a[i].len(),
                    attrs: attr_change(&a[i].attrs, &b[j].attrs),
                }),
                Step::Del(i) => out.push(Op::Delete(a[i].len())),
                Step::Ins(j) => {
                    let ins = match &b[j].kind {
                        Kind::Ch(c) => Insert::Text(c.to_string()),
                        Kind::Embed(e) => Insert::Embed(e.clone()),
                    };
                    out.push(Op::Insert {
                        ins,
                        attrs: b[j].attrs.clone(),
                    });
                }
            }
        }
        out.chop()
    }
}
