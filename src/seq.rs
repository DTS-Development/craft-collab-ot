//! 목록 op: 원소 삽입·삭제·이동(index 변환). 원소마다 문서 안에서 유일한 id를 가진다(word 블록, pdf 쪽 순서 등).
//! 같은 원소를 두 사람이 만지는 충돌은 id로 가린다: 같은 원소 삭제는 한 번만, 이동은 서버 순서가 늦은 쪽이 이긴다.
//!
//! 이동은 [삭제, 삽입] 두 단계로 풀어 변환한 뒤 다시 이동으로 묶는다(`to`는 원소를 뺀 목록에서의 위치).
use crate::core::{transform_seq, Doc, Invert, OtError, Transform};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Id = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum SeqOp {
    #[serde(rename = "ins")]
    Ins { i: usize, id: Id, v: Value },
    #[serde(rename = "del")]
    Del { i: usize, id: Id },
    /// from 위치의 원소를 빼서(그 뒤 목록에서) to 위치에 넣는다.
    #[serde(rename = "mov")]
    Mov { id: Id, from: usize, to: usize },
}

/// 변환용 한 단계(이동을 풀어 쓴 것).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Ins { i: usize, id: Id },
    Del { i: usize, id: Id },
}

impl Transform for Step {
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self> {
        match (*self, *other) {
            (Step::Ins { i, id }, Step::Ins { i: j, .. }) => {
                // 같은 자리 삽입은 서버 순서가 앞선 쪽이 앞에 온다
                vec![Step::Ins {
                    i: if j < i || (j == i && !a_first) {
                        i + 1
                    } else {
                        i
                    },
                    id,
                }]
            }
            (Step::Ins { i, id }, Step::Del { i: j, .. }) => vec![Step::Ins {
                i: if j < i { i - 1 } else { i },
                id,
            }],
            (Step::Del { i, id }, Step::Ins { i: j, .. }) => vec![Step::Del {
                i: if j <= i { i + 1 } else { i },
                id,
            }],
            (Step::Del { i, id }, Step::Del { i: j, id: id2 }) => {
                if j == i {
                    debug_assert_eq!(id, id2);
                    vec![] // 같은 원소를 둘 다 지웠다
                } else {
                    vec![Step::Del {
                        i: if j < i { i - 1 } else { i },
                        id,
                    }]
                }
            }
        }
    }
}

fn steps(op: &SeqOp) -> Vec<Step> {
    match *op {
        SeqOp::Ins { i, id, .. } => vec![Step::Ins { i, id }],
        SeqOp::Del { i, id } => vec![Step::Del { i, id }],
        SeqOp::Mov { id, from, to } => vec![Step::Del { i: from, id }, Step::Ins { i: to, id }],
    }
}

fn id_of(op: &SeqOp) -> Id {
    match *op {
        SeqOp::Ins { id, .. } | SeqOp::Del { id, .. } | SeqOp::Mov { id, .. } => id,
    }
}

impl Transform for SeqOp {
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self> {
        let same = id_of(self) == id_of(other)
            && !matches!(self, SeqOp::Ins { .. })
            && !matches!(other, SeqOp::Ins { .. });
        if same {
            match (self, other) {
                // 같은 원소를 이미 지웠다: self는 할 일이 없다
                (_, SeqOp::Del { .. }) => return vec![],
                // 지우려는 원소를 상대가 옮겼다: 옮겨진 자리에서 지운다
                (SeqOp::Del { id, .. }, SeqOp::Mov { to, .. }) => {
                    return vec![SeqOp::Del { i: *to, id: *id }]
                }
                // 둘 다 같은 원소를 옮겼다: 서버 순서가 늦은 쪽이 이긴다
                (SeqOp::Mov { id, to, .. }, SeqOp::Mov { to: other_to, .. }) => {
                    return if a_first {
                        vec![]
                    } else {
                        vec![SeqOp::Mov {
                            id: *id,
                            from: *other_to,
                            to: *to,
                        }]
                    };
                }
                _ => {}
            }
        }
        // 나머지는 단계로 풀어 변환한다(서로 다른 원소 사이의 위치 보정)
        let (a2, _) = transform_seq(&steps(self), &steps(other), a_first);
        match (self, a2.as_slice()) {
            (SeqOp::Ins { v, .. }, [Step::Ins { i, id }]) => vec![SeqOp::Ins {
                i: *i,
                id: *id,
                v: v.clone(),
            }],
            (SeqOp::Del { .. }, [Step::Del { i, id }]) => vec![SeqOp::Del { i: *i, id: *id }],
            (SeqOp::Mov { .. }, [Step::Del { i: f, id }, Step::Ins { i: t, .. }]) => {
                vec![SeqOp::Mov {
                    id: *id,
                    from: *f,
                    to: *t,
                }]
            }
            _ => vec![], // 같은 원소의 삽입 충돌 등 정상 흐름에서 오지 않는 조합
        }
    }
}

/// 원소 목록 문서.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqDoc(pub Vec<(Id, Value)>);

impl Doc for SeqDoc {
    type Op = SeqOp;
    fn apply(&mut self, op: &SeqOp) -> Result<(), OtError> {
        let bad = |m: &str| Err(OtError::Invalid(format!("seq {m}: {op:?}")));
        match op {
            SeqOp::Ins { i, id, v } => {
                if *i > self.0.len() {
                    return bad("삽입 위치");
                }
                self.0.insert(*i, (*id, v.clone()));
            }
            SeqOp::Del { i, id } => {
                if self.0.get(*i).map(|e| e.0) != Some(*id) {
                    return bad("삭제 대상");
                }
                self.0.remove(*i);
            }
            SeqOp::Mov { id, from, to } => {
                if self.0.get(*from).map(|e| e.0) != Some(*id) {
                    return bad("이동 대상");
                }
                let e = self.0.remove(*from);
                if *to > self.0.len() {
                    return bad("이동 위치");
                }
                self.0.insert(*to, e);
            }
        }
        Ok(())
    }
    fn canon(&self) -> Value {
        Value::Array(
            self.0
                .iter()
                .map(|(id, v)| serde_json::json!([id, v]))
                .collect(),
        )
    }
}

impl Invert<SeqDoc> for SeqOp {
    fn invert(&self, before: &SeqDoc) -> Vec<Self> {
        match self {
            SeqOp::Ins { i, id, .. } => vec![SeqOp::Del { i: *i, id: *id }],
            SeqOp::Del { i, id } => vec![SeqOp::Ins {
                i: *i,
                id: *id,
                v: before.0.get(*i).map(|e| e.1.clone()).unwrap_or(Value::Null),
            }],
            SeqOp::Mov { id, from, to } => vec![SeqOp::Mov {
                id: *id,
                from: *to,
                to: *from,
            }],
        }
    }
}
