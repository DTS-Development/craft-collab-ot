use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// embed 하나가 차지하는 길이(U+FFFC의 UTF-8 바이트 수).
pub const EMBED_LEN: usize = 3;

/// 속성 맵. 값 `null`은 retain에서 "속성 해제"를 뜻한다.
pub type Attrs = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq)]
pub enum Insert {
    Text(String),
    /// 개체(이미지·필드·책갈피 표지 등). JSON 객체여야 한다.
    Embed(Value),
}

impl Insert {
    pub fn len(&self) -> usize {
        match self {
            Insert::Text(s) => s.len(),
            Insert::Embed(_) => EMBED_LEN,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Retain { n: usize, attrs: Option<Attrs> },
    Insert { ins: Insert, attrs: Option<Attrs> },
    Delete(usize),
}

impl Op {
    pub fn len(&self) -> usize {
        match self {
            Op::Retain { n, .. } => *n,
            Op::Insert { ins, .. } => ins.len(),
            Op::Delete(n) => *n,
        }
    }
    fn attrs(&self) -> Option<&Attrs> {
        match self {
            Op::Retain { attrs, .. } | Op::Insert { attrs, .. } => attrs.as_ref(),
            Op::Delete(_) => None,
        }
    }
    fn is_insert(&self) -> bool {
        matches!(self, Op::Insert { .. })
    }
    fn is_delete(&self) -> bool {
        matches!(self, Op::Delete(_))
    }
}

/// 빈 속성 맵은 None으로 통일(같은 값이 같게 비교되도록).
fn norm(a: Option<Attrs>) -> Option<Attrs> {
    a.filter(|m| !m.is_empty())
}

/// 속성 합성: `b`가 `a` 뒤에 적용. `keep_null`이면 해제 표시(null)를 남긴다(retain끼리).
fn compose_attrs(a: Option<&Attrs>, b: Option<&Attrs>, keep_null: bool) -> Option<Attrs> {
    let mut out: Attrs = b
        .map(|b| {
            b.iter()
                .filter(|(_, v)| keep_null || !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(a) = a {
        for (k, v) in a {
            if !b.is_some_and(|b| b.contains_key(k)) {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    norm(Some(out))
}

/// 속성 변환: `a`가 먼저(우선) 적용된 뒤의 `b`. priority면 a가 이긴 키를 b에서 뺀다.
fn transform_attrs(a: Option<&Attrs>, b: Option<&Attrs>, priority: bool) -> Option<Attrs> {
    let b = b?;
    let a = match a {
        Some(a) if priority => a,
        _ => return norm(Some(b.clone())),
    };
    norm(Some(
        b.iter()
            .filter(|(k, _)| !a.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    ))
}

/// `attr`을 되돌리는 속성(`base`는 적용 전 값).
fn invert_attrs(attr: &Attrs, base: Option<&Attrs>) -> Option<Attrs> {
    let mut out = Attrs::new();
    for (k, v) in attr {
        let old = base.and_then(|b| b.get(k)).filter(|v| !v.is_null());
        let new = if v.is_null() { None } else { Some(v) };
        if old != new {
            out.insert(k.clone(), old.cloned().unwrap_or(Value::Null));
        }
    }
    norm(Some(out))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delta {
    ops: Vec<Op>,
}

/// 줄 끝까지 소진된 쪽은 "무한 retain"으로 본다(Quill과 같음).
struct Iter<'a> {
    ops: &'a [Op],
    idx: usize,
    off: usize,
}

impl<'a> Iter<'a> {
    fn new(d: &'a Delta) -> Self {
        Iter { ops: &d.ops, idx: 0, off: 0 }
    }
    fn has_next(&self) -> bool {
        self.idx < self.ops.len()
    }
    fn peek_len(&self) -> usize {
        self.ops.get(self.idx).map_or(usize::MAX, |o| o.len() - self.off)
    }
    fn peek(&self) -> Option<&Op> {
        self.ops.get(self.idx)
    }
    fn peek_is_insert(&self) -> bool {
        self.peek().is_some_and(Op::is_insert)
    }
    fn peek_is_delete(&self) -> bool {
        self.peek().is_some_and(Op::is_delete)
    }
    /// 최대 `len`만큼 잘라 가져온다. 소진 상태면 속성 없는 retain.
    fn next(&mut self, len: usize) -> Op {
        let Some(op) = self.ops.get(self.idx) else {
            return Op::Retain { n: len, attrs: None };
        };
        let remain = op.len() - self.off;
        let take = len.min(remain);
        let out = match op {
            Op::Retain { attrs, .. } => Op::Retain { n: take, attrs: attrs.clone() },
            Op::Delete(_) => Op::Delete(take),
            Op::Insert { ins, attrs } => {
                let ins = match ins {
                    Insert::Text(s) => {
                        let piece = s
                            .get(self.off..self.off + take)
                            .expect("글자 경계가 아닌 길이로 delta를 잘랐다");
                        Insert::Text(piece.to_string())
                    }
                    Insert::Embed(v) => {
                        assert!(self.off == 0 && take == EMBED_LEN, "embed는 나눌 수 없다");
                        Insert::Embed(v.clone())
                    }
                };
                Op::Insert { ins, attrs: attrs.clone() }
            }
        };
        if take == remain {
            self.idx += 1;
            self.off = 0;
        } else {
            self.off += take;
        }
        out
    }
}

impl Delta {
    pub fn new() -> Self {
        Delta::default()
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 만들기 쉬운 빌더 함수들. 빈 길이는 무시, 인접 op는 합쳐진다.
    pub fn retain(mut self, n: usize, attrs: Option<Attrs>) -> Self {
        self.push(Op::Retain { n, attrs: norm(attrs) });
        self
    }
    pub fn insert(mut self, text: &str, attrs: Option<Attrs>) -> Self {
        self.push(Op::Insert { ins: Insert::Text(text.to_string()), attrs: norm(attrs) });
        self
    }
    pub fn insert_embed(mut self, v: Value, attrs: Option<Attrs>) -> Self {
        self.push(Op::Insert { ins: Insert::Embed(v), attrs: norm(attrs) });
        self
    }
    pub fn delete(mut self, n: usize) -> Self {
        self.push(Op::Delete(n));
        self
    }

    /// 정규형 유지: 길이 0 무시, 같은 종류·속성은 합치고, 삽입은 삭제 앞에 둔다.
    pub fn push(&mut self, op: Op) {
        if op.len() == 0 {
            return;
        }
        let op = match op {
            Op::Retain { n, attrs } => Op::Retain { n, attrs: norm(attrs) },
            Op::Insert { ins, attrs } => Op::Insert { ins, attrs: norm(attrs) },
            d => d,
        };
        if let Some(last) = self.ops.last_mut() {
            match (last, &op) {
                (Op::Delete(a), Op::Delete(b)) => {
                    *a += b;
                    return;
                }
                (Op::Delete(_), Op::Insert { .. }) => {
                    let del = self.ops.pop().unwrap();
                    self.push(op);
                    self.push(del);
                    return;
                }
                (Op::Retain { n: a, attrs: x }, Op::Retain { n: b, attrs: y }) if x == y => {
                    *a += b;
                    return;
                }
                (
                    Op::Insert { ins: Insert::Text(a), attrs: x },
                    Op::Insert { ins: Insert::Text(b), attrs: y },
                ) if x == y => {
                    a.push_str(b);
                    return;
                }
                _ => {}
            }
        }
        self.ops.push(op);
    }

    /// 끝의 속성 없는 retain을 지운다.
    pub fn chop(mut self) -> Self {
        if let Some(Op::Retain { attrs: None, .. }) = self.ops.last() {
            self.ops.pop();
        }
        self
    }

    pub fn from_ops(ops: Vec<Op>) -> Self {
        let mut d = Delta::new();
        for op in ops {
            d.push(op);
        }
        d
    }

    /// 문서(삽입만 있는 delta)의 길이, 또는 변경 delta가 적용될 기준 문서의 최소 길이.
    pub fn base_len(&self) -> usize {
        self.ops
            .iter()
            .map(|o| if o.is_insert() { 0 } else { o.len() })
            .sum()
    }

    /// 적용 뒤의 길이.
    pub fn target_len(&self) -> usize {
        self.ops
            .iter()
            .map(|o| if o.is_delete() { 0 } else { o.len() })
            .sum()
    }

    /// `self` 다음에 `other`를 적용한 것과 같은 하나의 delta.
    pub fn compose(&self, other: &Delta) -> Delta {
        let (mut a, mut b) = (Iter::new(self), Iter::new(other));
        let mut out = Delta::new();
        while a.has_next() || b.has_next() {
            if b.peek_is_insert() {
                out.push(b.next(usize::MAX));
            } else if a.peek_is_delete() {
                out.push(a.next(usize::MAX));
            } else {
                let len = a.peek_len().min(b.peek_len());
                let (ao, bo) = (a.next(len), b.next(len));
                match bo {
                    Op::Retain { attrs: ba, .. } => match ao {
                        Op::Retain { attrs: aa, .. } => out.push(Op::Retain {
                            n: len,
                            attrs: compose_attrs(aa.as_ref(), ba.as_ref(), true),
                        }),
                        Op::Insert { ins, attrs: aa } => out.push(Op::Insert {
                            ins,
                            attrs: compose_attrs(aa.as_ref(), ba.as_ref(), false),
                        }),
                        Op::Delete(_) => unreachable!(),
                    },
                    Op::Delete(_) => {
                        // 앞 삽입을 지우면 서로 상쇄, retain을 지우면 삭제로 남는다.
                        if matches!(ao, Op::Retain { .. }) {
                            out.push(Op::Delete(len));
                        }
                    }
                    Op::Insert { .. } => unreachable!(),
                }
            }
        }
        out.chop()
    }

    /// `self`가 먼저 적용된 문서에 맞게 `other`를 바꾼다.
    /// `priority`가 true면 같은 자리 삽입·같은 키 속성에서 `self`가 앞선다(rev가 더 이른 쪽).
    pub fn transform(&self, other: &Delta, priority: bool) -> Delta {
        let (mut a, mut b) = (Iter::new(self), Iter::new(other));
        let mut out = Delta::new();
        while a.has_next() || b.has_next() {
            if a.peek_is_insert() && (priority || !b.peek_is_insert()) {
                let n = a.next(usize::MAX).len();
                out.push(Op::Retain { n, attrs: None });
            } else if b.peek_is_insert() {
                out.push(b.next(usize::MAX));
            } else {
                let len = a.peek_len().min(b.peek_len());
                let (ao, bo) = (a.next(len), b.next(len));
                if ao.is_delete() {
                    continue; // 이미 지워진 구간의 b 연산은 사라진다
                } else if bo.is_delete() {
                    out.push(bo);
                } else {
                    out.push(Op::Retain {
                        n: len,
                        attrs: transform_attrs(ao.attrs(), bo.attrs(), priority),
                    });
                }
            }
        }
        out.chop()
    }

    /// `self`를 `base`(적용 전 문서, 삽입만 있는 delta)에 적용한 것의 역.
    pub fn invert(&self, base: &Delta) -> Delta {
        let mut inv = Delta::new();
        let mut at = 0usize;
        for op in &self.ops {
            match op {
                Op::Insert { .. } => inv.push(Op::Delete(op.len())),
                Op::Retain { n, attrs: None } => {
                    inv.push(Op::Retain { n: *n, attrs: None });
                    at += n;
                }
                Op::Delete(n) => {
                    for piece in base.slice(at, at + n).ops {
                        inv.push(piece);
                    }
                    at += n;
                }
                Op::Retain { n, attrs: Some(attrs) } => {
                    for piece in base.slice(at, at + n).ops {
                        let len = piece.len();
                        inv.push(Op::Retain { n: len, attrs: invert_attrs(attrs, piece.attrs()) });
                    }
                    at += n;
                }
            }
        }
        inv.chop()
    }

    /// `[start, end)` 구간을 잘라 낸다(문서 delta용).
    pub fn slice(&self, start: usize, end: usize) -> Delta {
        let mut out = Delta::new();
        let mut it = Iter::new(self);
        let mut pos = 0;
        while pos < end && it.has_next() {
            if pos < start {
                let skip = (start - pos).min(it.peek_len());
                pos += it.next(skip).len();
            } else {
                let op = it.next(end - pos);
                pos += op.len();
                out.push(op);
            }
        }
        out
    }

    /// 편집 앞 위치(바이트)를 이 delta 뒤의 위치로 옮긴다. 같은 자리 삽입은
    /// `priority`면 위치를 밀지 않는다(캐럿이 삽입 앞에 남음).
    pub fn transform_position(&self, index: usize, priority: bool) -> usize {
        let mut index = index;
        let mut offset = 0usize;
        for op in &self.ops {
            if offset > index {
                break;
            }
            let len = op.len();
            match op {
                Op::Delete(_) => {
                    index -= len.min(index - offset);
                    continue;
                }
                Op::Insert { .. } if offset < index || !priority => index += len,
                _ => {}
            }
            offset += len;
        }
        index
    }

    /// 문서 delta(`self`, 삽입만 있음)에 변경 delta를 적용해도 되는지 검사한다
    /// (길이 초과·글자 경계 위반). 원격에서 온 delta를 적용하기 전에 쓴다.
    pub fn check_on(&self, change: &Delta) -> Result<(), String> {
        let total: usize = self.ops.iter().map(Op::len).sum();
        if change.base_len() > total {
            return Err(format!("delta가 문서 길이 {total}보다 길다({})", change.base_len()));
        }
        // 변경이 문서를 가르는 지점(누적 위치)이 모두 글자·embed 경계인지 한 번에 훑는다.
        let mut cuts = Vec::new();
        let mut at = 0usize;
        for op in change.ops.iter().filter(|o| !o.is_insert()) {
            at += op.len();
            cuts.push(at);
        }
        let mut ci = 0usize;
        let mut pos = 0usize;
        for d in &self.ops {
            let l = d.len();
            while ci < cuts.len() && cuts[ci] <= pos {
                ci += 1;
            }
            while ci < cuts.len() && cuts[ci] < pos + l {
                if !boundary(d, cuts[ci] - pos) {
                    return Err("delta가 글자 경계 안쪽을 가리킨다".into());
                }
                ci += 1;
            }
            pos += l;
        }
        Ok(())
    }
}

fn boundary(op: &Op, off: usize) -> bool {
    match op {
        Op::Insert { ins: Insert::Text(s), .. } => s.is_char_boundary(off),
        Op::Insert { ins: Insert::Embed(_), .. } => false,
        _ => true,
    }
}

// ---- JSON (Quill 호환 모양: {"ops":[{"insert":"a","attributes":{}},{"retain":3},{"delete":2}]}) ----

#[derive(Serialize, Deserialize)]
struct RawOp {
    #[serde(skip_serializing_if = "Option::is_none")]
    insert: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retain: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delete: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<Attrs>,
}

impl Serialize for Op {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let raw = match self {
            Op::Retain { n, attrs } => RawOp { insert: None, retain: Some(*n), delete: None, attributes: attrs.clone() },
            Op::Delete(n) => RawOp { insert: None, retain: None, delete: Some(*n), attributes: None },
            Op::Insert { ins, attrs } => RawOp {
                insert: Some(match ins {
                    Insert::Text(t) => Value::String(t.clone()),
                    Insert::Embed(v) => v.clone(),
                }),
                retain: None,
                delete: None,
                attributes: attrs.clone(),
            },
        };
        raw.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Op {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let raw = RawOp::deserialize(d)?;
        let attrs = norm(raw.attributes);
        match (raw.insert, raw.retain, raw.delete) {
            (Some(Value::String(t)), None, None) if !t.is_empty() => {
                Ok(Op::Insert { ins: Insert::Text(t), attrs })
            }
            (Some(v @ Value::Object(_)), None, None) => Ok(Op::Insert { ins: Insert::Embed(v), attrs }),
            (None, Some(n), None) if n > 0 => Ok(Op::Retain { n, attrs }),
            (None, None, Some(n)) if n > 0 && attrs.is_none() => Ok(Op::Delete(n)),
            _ => Err(D::Error::custom("올바르지 않은 delta op")),
        }
    }
}

impl Serialize for Delta {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct W<'a> {
            ops: &'a [Op],
        }
        W { ops: &self.ops }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Delta {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct W {
            ops: Vec<Op>,
        }
        Ok(Delta::from_ops(W::deserialize(d)?.ops))
    }
}
