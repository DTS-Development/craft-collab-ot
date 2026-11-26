//! 수렴·역연산·합성 퍼즈 시험. 외부 크레이트 없이 xorshift로 무작위를 만든다.
use super::*;
use crate::core::{Doc, Transform};
use serde_json::{json, Value};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const CHARS: [&str; 5] = ["a", "b", "é", "日", "😀"];

fn rand_attrs(r: &mut Rng, allow_null: bool) -> Option<Attrs> {
    if r.below(2) == 0 {
        return None;
    }
    let mut m = Attrs::new();
    for k in ["b", "c"] {
        match r.below(4) {
            0 => {
                m.insert(k.into(), json!(r.below(3)));
            }
            1 if allow_null => {
                m.insert(k.into(), Value::Null);
            }
            _ => {}
        }
    }
    Some(m)
}

fn rand_insert(r: &mut Rng, attrs: Option<Attrs>) -> (Insert, Option<Attrs>) {
    if r.below(6) == 0 {
        (Insert::Embed(json!({"obj": r.below(3)})), attrs)
    } else {
        let mut s = String::new();
        for _ in 0..1 + r.below(3) {
            s.push_str(CHARS[r.below(CHARS.len())]);
        }
        (Insert::Text(s), attrs)
    }
}

/// 무작위 문서(삽입만).
fn rand_doc(r: &mut Rng) -> Delta {
    let mut d = Delta::new();
    for _ in 0..r.below(5) {
        let a = rand_attrs(r, false);
        let (ins, attrs) = rand_insert(r, a);
        d.push(Op::Insert { ins, attrs });
    }
    d
}

/// 문서의 글자 단위(경계) 길이 목록.
fn atoms(doc: &Delta) -> Vec<usize> {
    let mut v = Vec::new();
    for op in doc.ops() {
        if let Op::Insert { ins, .. } = op {
            match ins {
                Insert::Text(s) => v.extend(s.chars().map(char::len_utf8)),
                Insert::Embed(_) => v.push(EMBED_LEN),
            }
        }
    }
    v
}

/// 문서에 대한 무작위 변경.
fn rand_change(r: &mut Rng, doc: &Delta) -> Delta {
    let at = atoms(doc);
    let mut d = Delta::new();
    let mut i = 0;
    while i < at.len() || r.below(3) == 0 {
        match r.below(4) {
            0 => {
                let a = rand_attrs(r, false);
                let (ins, attrs) = rand_insert(r, a);
                d.push(Op::Insert { ins, attrs });
            }
            1 if i < at.len() => {
                let k = 1 + r.below((at.len() - i).min(3));
                d.push(Op::Delete(at[i..i + k].iter().sum()));
                i += k;
            }
            2 if i < at.len() => {
                let k = 1 + r.below((at.len() - i).min(3));
                d.push(Op::Retain { n: at[i..i + k].iter().sum(), attrs: rand_attrs(r, true) });
                i += k;
            }
            3 if i < at.len() => {
                let k = 1 + r.below((at.len() - i).min(3));
                d.push(Op::Retain { n: at[i..i + k].iter().sum(), attrs: None });
                i += k;
            }
            _ => {
                if i >= at.len() {
                    break;
                }
            }
        }
    }
    d.chop()
}

#[test]
fn tp1_converges() {
    let mut r = Rng(0x9e3779b97f4a7c15);
    for _ in 0..20000 {
        let doc = rand_doc(&mut r);
        let (a, b) = (rand_change(&mut r, &doc), rand_change(&mut r, &doc));
        let left = doc.compose(&a).compose(&a.transform(&b, true));
        let right = doc.compose(&b).compose(&b.transform(&a, false));
        assert_eq!(left, right, "doc={doc:?}\na={a:?}\nb={b:?}");
    }
}

#[test]
fn invert_restores() {
    let mut r = Rng(0xdeadbeefcafef00d);
    for _ in 0..20000 {
        let doc = rand_doc(&mut r);
        let a = rand_change(&mut r, &doc);
        let after = doc.compose(&a);
        assert_eq!(after.compose(&a.invert(&doc)), doc, "doc={doc:?}\na={a:?}");
    }
}

#[test]
fn compose_is_associative() {
    let mut r = Rng(0x1234567890abcdef);
    for _ in 0..10000 {
        let doc = rand_doc(&mut r);
        let a = rand_change(&mut r, &doc);
        let d1 = doc.compose(&a);
        let b = rand_change(&mut r, &d1);
        assert_eq!(doc.compose(&a.compose(&b)), d1.compose(&b), "doc={doc:?}\na={a:?}\nb={b:?}");
    }
}

#[test]
fn changes_pass_check_on() {
    let mut r = Rng(42);
    for _ in 0..5000 {
        let doc = rand_doc(&mut r);
        let a = rand_change(&mut r, &doc);
        assert!(doc.check_on(&a).is_ok(), "{doc:?} {a:?}");
    }
    let doc = Delta::new().insert("é", None);
    assert!(doc.check_on(&Delta::new().retain(1, None)).is_err());
    assert!(doc.check_on(&Delta::new().retain(3, None)).is_err());
    let emb = Delta::new().insert_embed(json!({"o":1}), None);
    assert!(emb.check_on(&Delta::new().delete(1)).is_err());
}

#[test]
fn position_follows_edits() {
    // "abc" 맨 앞에 "XY" 삽입: 위치 1은 3으로.
    let ins = Delta::new().insert("XY", None);
    assert_eq!(ins.transform_position(1, false), 3);
    // 같은 자리 삽입은 priority가 있으면 밀지 않는다.
    assert_eq!(ins.transform_position(0, true), 0);
    assert_eq!(ins.transform_position(0, false), 2);
    // 앞 구간 삭제.
    let del = Delta::new().delete(2);
    assert_eq!(del.transform_position(5, false), 3);
    assert_eq!(del.transform_position(1, false), 0);
}

#[test]
fn json_round_trip() {
    let d = Delta::new()
        .retain(2, Some([("b".to_string(), json!(1))].into()))
        .insert("é", None)
        .insert_embed(json!({"img": "h"}), None)
        .delete(3);
    let s = serde_json::to_string(&d).unwrap();
    let back: Delta = serde_json::from_str(&s).unwrap();
    assert_eq!(d, back);
    assert!(serde_json::from_str::<Delta>(r#"{"ops":[{"retain":0}]}"#).is_err());
    assert!(serde_json::from_str::<Delta>(r#"{"ops":[{"insert":"a","retain":1}]}"#).is_err());
}

#[test]
fn diff_composes_back() {
    let mut r = Rng(777);
    for _ in 0..20000 {
        let a = rand_doc(&mut r);
        let b = rand_doc(&mut r);
        let d = a.diff(&b);
        assert_eq!(a.compose(&d), b, "a={a:?}\nb={b:?}\nd={d:?}");
        // 같은 문서끼리는 변경이 없다.
        assert!(a.diff(&a).is_empty());
    }
    // 가운데 한 글자 바뀜은 그 글자만 건드린다.
    let a = Delta::new().insert("hello", None);
    let b = Delta::new().insert("heXlo", None);
    assert_eq!(a.diff(&b), Delta::new().retain(2, None).insert("X", None).delete(1));
}

// ---- 서버 순서 하네스(sim) 연동 ----

impl crate::sim::SimDoc for TextDoc {
    fn initial() -> Self {
        TextDoc::default()
    }
    fn random_op(&self, rng: &mut crate::sim::Rng) -> Option<TextOp> {
        let mut r = Rng(rng.next() | 1);
        let key = ["body", "cell"][r.below(2)].to_string();
        let doc = self.0.get(&key).cloned().unwrap_or_default();
        let delta = rand_change(&mut r, &doc);
        (!delta.is_empty()).then_some(TextOp { key, delta })
    }
}

#[test]
fn text_op_wire_and_transform() {
    let op = TextOp { key: "k".into(), delta: Delta::new().retain(2, None).insert("é", None) };
    let s = serde_json::to_string(&op).unwrap();
    assert!(s.contains(r#""t":"text""#));
    assert_eq!(serde_json::from_str::<TextOp>(&s).unwrap(), op);
    // 다른 키는 변환하지 않는다.
    let other = TextOp { key: "z".into(), delta: Delta::new().insert("x", None) };
    assert_eq!(op.transform(&other, true), vec![op.clone()]);
    // 같은 자리 삽입: 앞선 쪽이 먼저 들어간다.
    let a = TextOp { key: "k".into(), delta: Delta::new().insert("A", None) };
    let b = TextOp { key: "k".into(), delta: Delta::new().insert("B", None) };
    let mut d1 = TextDoc::default();
    d1.apply(&a).unwrap();
    d1.apply(&b.transform(&a, false)[0]).unwrap();
    let mut d2 = TextDoc::default();
    d2.apply(&b).unwrap();
    d2.apply(&a.transform(&b, true)[0]).unwrap();
    assert_eq!(d1.canon(), d2.canon());
}

#[test]
fn text_doc_converges_under_the_server_order_harness() {
    // 서식 속성·embed·한글·이모지는 rand_* 생성기가 섞는다.
    for clients in 2..=5 {
        crate::sim::run_fuzz::<TextDoc>(0..200, clients, 40).unwrap();
    }
}
