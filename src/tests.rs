//! reg·seq 변환 시험: TP1 전수(작은 목록) + 서버 순서 수렴 퍼즈 + 역 op.
use crate::core::{Doc, Invert};
use crate::reg::{RegDoc, RegOp};
use crate::seq::{SeqDoc, SeqOp};
use crate::sim::{run_fuzz, Rng, SimDoc};
use serde_json::{json, Value};

fn apply_all<D: Doc>(d: &D, ops: &[D::Op]) -> Result<D, String> {
    let mut d = d.clone();
    for o in ops {
        d.apply(o).map_err(|e| e.to_string())?;
    }
    Ok(d)
}

/// a가 서버 순서에서 앞, b가 뒤. 양쪽 경로가 같은 정규형이 되어야 한다.
fn tp1<D: Doc>(s: &D, a: &D::Op, b: &D::Op) -> Result<(), String> {
    use crate::core::Transform;
    let b2 = b.transform(a, false);
    let a2 = a.transform(b, true);
    let p1 = apply_all(&apply_all(s, std::slice::from_ref(a))?, &b2)?;
    let p2 = apply_all(&apply_all(s, std::slice::from_ref(b))?, &a2)?;
    if p1.canon() == p2.canon() {
        Ok(())
    } else {
        Err(format!("TP1 실패\n  s={}\n  a={a:?}\n  b={b:?}\n  a->b'={}\n  b->a'={}", s.canon(), p1.canon(), p2.canon()))
    }
}

fn seq_doc(n: usize) -> SeqDoc {
    SeqDoc((0..n as u64).map(|i| (i + 1, json!(i))).collect())
}

/// 이 문서에서 만들 수 있는 모든 op(새 원소 id는 `fresh`)
fn all_seq_ops(d: &SeqDoc, fresh: u64) -> Vec<SeqOp> {
    let n = d.0.len();
    let mut v = vec![];
    for i in 0..=n {
        v.push(SeqOp::Ins { i, id: fresh, v: json!("x") });
    }
    for i in 0..n {
        v.push(SeqOp::Del { i, id: d.0[i].0 });
        for to in 0..n {
            v.push(SeqOp::Mov { id: d.0[i].0, from: i, to });
        }
    }
    v
}

#[test]
fn seq_tp1_exhaustive() {
    for n in 0..=5 {
        let s = seq_doc(n);
        // 서로 다른 클라이언트가 만든 새 원소는 id가 다르다
        let (xs, ys) = (all_seq_ops(&s, 100), all_seq_ops(&s, 200));
        for a in &xs {
            for b in &ys {
                tp1(&s, a, b).unwrap_or_else(|e| panic!("{e}"));
            }
        }
    }
}

#[test]
fn reg_tp1_exhaustive() {
    let mut s = RegDoc::default();
    s.apply(&RegOp::set("a", json!(1))).unwrap();
    let ops: Vec<RegOp> = ["a", "b"].iter().flat_map(|k| vec![RegOp::set(*k, json!(7)), RegOp::set(*k, json!(8)), RegOp::del(*k)]).collect();
    for a in &ops {
        for b in &ops {
            tp1(&s, a, b).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}

impl SimDoc for SeqDoc {
    fn initial() -> Self {
        seq_doc(3)
    }
    fn random_op(&self, rng: &mut Rng) -> Option<SeqOp> {
        let fresh = 1_000 + rng.next_u64() % 1_000_000_000_000;
        let n = self.0.len();
        Some(match rng.below(3) {
            0 => SeqOp::Ins { i: rng.below(n + 1), id: fresh, v: json!(fresh) },
            1 if n > 0 => {
                let i = rng.below(n);
                SeqOp::Del { i, id: self.0[i].0 }
            }
            _ if n > 0 => {
                let i = rng.below(n);
                SeqOp::Mov { id: self.0[i].0, from: i, to: rng.below(n) }
            }
            _ => return None,
        })
    }
}

impl SimDoc for RegDoc {
    fn initial() -> Self {
        RegDoc::default()
    }
    fn random_op(&self, rng: &mut Rng) -> Option<RegOp> {
        let key = format!("k{}", rng.below(4));
        Some(if rng.chance(1, 4) { RegOp::del(key) } else { RegOp::set(key, json!(rng.below(1000))) })
    }
}

#[test]
fn seq_fuzz_converges() {
    for clients in 2..=5 {
        if let Err(f) = run_fuzz::<SeqDoc>(0..200, clients, 40) {
            panic!("{f:?}");
        }
    }
}

#[test]
fn reg_fuzz_converges() {
    for clients in 2..=5 {
        if let Err(f) = run_fuzz::<RegDoc>(0..200, clients, 40) {
            panic!("{f:?}");
        }
    }
}

#[test]
fn seq_invert_roundtrip() {
    let mut rng = Rng::new(7);
    for _ in 0..500 {
        let mut d = seq_doc(rng.below(5));
        for _ in 0..6 {
            let Some(op) = d.random_op(&mut rng) else { continue };
            let before = d.clone();
            d.apply(&op).unwrap();
            let inv = op.invert(&before);
            let back = apply_all(&d, &inv).unwrap();
            assert_eq!(back.canon(), before.canon(), "{op:?}");
        }
    }
}

#[test]
fn reg_invert_roundtrip() {
    let mut rng = Rng::new(9);
    let mut d = RegDoc::default();
    for _ in 0..300 {
        let op = d.random_op(&mut rng).unwrap();
        let before = d.clone();
        d.apply(&op).unwrap();
        let back = apply_all(&d, &op.invert(&before)).unwrap();
        assert_eq!(back.canon(), before.canon());
    }
}

#[test]
fn wire_shapes() {
    let v: Value = serde_json::to_value(RegOp::set("k", json!(1))).unwrap();
    assert_eq!(v, json!({"t": "reg", "key": "k", "v": 1}));
    assert_eq!(serde_json::to_value(RegOp::del("k")).unwrap(), json!({"t": "reg", "key": "k"}));
    let m = SeqOp::Mov { id: 3, from: 1, to: 2 };
    assert_eq!(serde_json::to_value(&m).unwrap(), json!({"t": "mov", "id": 3, "from": 1, "to": 2}));
    let back: SeqOp = serde_json::from_value(json!({"t": "ins", "i": 0, "id": 9, "v": "a"})).unwrap();
    assert_eq!(back, SeqOp::Ins { i: 0, id: 9, v: json!("a") });
}

/// 순차 묶음끼리도 같은 상태로 수렴한다(transform_seq 격자).
#[test]
fn seq_batches_converge() {
    use crate::core::transform_seq;
    let mut rng = Rng::new(3);
    for it in 0..3000 {
        let s = seq_doc(rng.below(5));
        let mk = |rng: &mut Rng| {
            let mut d = s.clone();
            let mut v = vec![];
            for _ in 0..1 + rng.below(3) {
                if let Some(o) = d.random_op(rng) {
                    d.apply(&o).unwrap();
                    v.push(o);
                }
            }
            v
        };
        let a = mk(&mut rng);
        let b = mk(&mut rng);
        let (a2, b2) = transform_seq(&a, &b, true);
        let p1 = apply_all(&apply_all(&s, &a).unwrap(), &b2).unwrap();
        let p2 = apply_all(&apply_all(&s, &b).unwrap(), &a2).unwrap();
        assert_eq!(p1.canon(), p2.canon(), "iter {it}\n s={}\n a={a:?}\n b={b:?}", s.canon());
    }
}

/// 반복문 `transform_seq`는 옛 재귀와 같은 결과를 낸다(쪼개짐·사라짐이 있는 seq 묶음과 reg 묶음).
#[test]
fn transform_seq_loop_equals_the_old_recursion() {
    use crate::core::{transform_seq, transform_seq_rec};
    let mut rng = Rng::new(21);
    for it in 0..4000 {
        let s = seq_doc(rng.below(6));
        let mk = |rng: &mut Rng| {
            let mut d = s.clone();
            let mut v = vec![];
            for _ in 0..1 + rng.below(6) {
                if let Some(o) = d.random_op(rng) {
                    d.apply(&o).unwrap();
                    v.push(o);
                }
            }
            v
        };
        let (a, b) = (mk(&mut rng), mk(&mut rng));
        for first in [true, false] {
            assert_eq!(transform_seq(&a, &b, first), transform_seq_rec(&a, &b, first), "iter {it} seq\n a={a:?}\n b={b:?}");
        }
        let regs = |rng: &mut Rng| -> Vec<RegOp> {
            (0..1 + rng.below(8)).map(|_| RegOp { key: format!("k{}", rng.below(4)), value: rng.chance(1, 2).then(|| json!(rng.below(9))) }).collect()
        };
        let (ra, rb) = (regs(&mut rng), regs(&mut rng));
        for first in [true, false] {
            assert_eq!(transform_seq(&ra, &rb, first), transform_seq_rec(&ra, &rb, first), "iter {it} reg");
        }
    }
}

/// 큰 묶음에서 스택이 넘치지 않는다(원격 묶음 하나로 편집기가 죽던 문제). 스레드 스택을 1MiB로 줄여 wasm과 맞춘다.
#[test]
fn transform_seq_does_not_overflow_the_stack_on_big_bundles() {
    use crate::core::transform_seq;
    let go = || {
        let reg = |n: usize, p: &str| -> Vec<RegOp> { (0..n).map(|i| RegOp { key: format!("{p}{}", i % 50), value: Some(json!(i)) }).collect() };
        for (n, m) in [(10_000, 1), (1, 10_000), (2_000, 2_000), (5_000, 1)] {
            let (a, b) = (reg(n, "k"), reg(m, "k"));
            let t = std::time::Instant::now();
            let (a2, b2) = transform_seq(&a, &b, true);
            eprintln!("transform_seq {n}x{m}: {:?}", t.elapsed());
            assert!(a2.len() <= n && b2.len() <= m);
        }
        // 키가 겹치지 않으면 변환 없이 지나가 빠르다.
        let (a, b) = (reg(2_000, "a"), reg(2_000, "b"));
        let (a2, b2) = transform_seq(&a, &b, true);
        assert_eq!((a2.len(), b2.len()), (2_000, 2_000));
    };
    std::thread::Builder::new().stack_size(1 << 20).spawn(go).unwrap().join().unwrap();
}

/// 서로 건드리지 않는 묶음은 op마다 반대편 묶음을 새로 복사하지 않는다(n×m 복사): 키가 모두 다른 큰 묶음이 곧바로 끝난다.
#[test]
fn transform_seq_skips_independent_ops_without_copying() {
    use crate::core::transform_seq;
    let reg = |n: usize, p: &str| -> Vec<RegOp> { (0..n).map(|i| RegOp { key: format!("{p}{i}"), value: Some(json!(i)) }).collect() };
    let (a, b) = (reg(5_000, "a"), reg(5_000, "b"));
    let t = std::time::Instant::now();
    let (a2, b2) = transform_seq(&a, &b, true);
    assert_eq!((a2, b2), (a, b));
    eprintln!("independent 5000x5000: {:?}", t.elapsed());
}
