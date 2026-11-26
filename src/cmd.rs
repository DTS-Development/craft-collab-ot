//! 명령 op: 이름 + 매개변수를 받는 쪽이 같은 순서로 다시 실행한다(grid 행·열 구조 편집 같은 것).
//! 변환 규칙은 앱이 정한다: [`CmdRules`]를 구현하고 `CmdOp<규칙>`을 쓴다. 규칙이 없으면(`NoRules`) 변환하지 않는다
//! (서로 교환되는 명령, 예: 서로 다른 셀 쓰기).
use crate::core::Transform;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::marker::PhantomData;

/// 앱의 명령 변환 규칙. TP1을 지켜야 한다(`a`를 `b` 뒤로: 같은 상태에서 갈라진 두 명령).
pub trait CmdRules: 'static {
    fn transform(a: &CmdBody, b: &CmdBody, a_first: bool) -> Vec<CmdBody>;
}

/// 변환하지 않는 규칙(모든 명령이 서로 교환된다).
pub struct NoRules;
impl CmdRules for NoRules {
    fn transform(a: &CmdBody, _b: &CmdBody, _a_first: bool) -> Vec<CmdBody> {
        vec![a.clone()]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CmdBody {
    #[serde(rename = "c")]
    pub name: String,
    #[serde(rename = "a", default)]
    pub args: Value,
}

/// wire 모양: `{"t":"cmd","c":"insert_row","a":{...}}`
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct CmdOp<R: CmdRules = NoRules> {
    #[serde(rename = "t")]
    tag: CmdTag,
    #[serde(flatten)]
    pub body: CmdBody,
    #[serde(skip)]
    _rules: PhantomData<fn() -> R>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
enum CmdTag {
    #[serde(rename = "cmd")]
    Cmd,
}

impl<R: CmdRules> CmdOp<R> {
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self::from_body(CmdBody { name: name.into(), args })
    }
    pub fn from_body(body: CmdBody) -> Self {
        CmdOp { tag: CmdTag::Cmd, body, _rules: PhantomData }
    }
}

impl<R: CmdRules> Clone for CmdOp<R> {
    fn clone(&self) -> Self {
        Self::from_body(self.body.clone())
    }
}
impl<R: CmdRules> fmt::Debug for CmdOp<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cmd {}({})", self.body.name, self.body.args)
    }
}
impl<R: CmdRules> PartialEq for CmdOp<R> {
    fn eq(&self, o: &Self) -> bool {
        self.body == o.body
    }
}

impl<R: CmdRules> Transform for CmdOp<R> {
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self> {
        R::transform(&self.body, &other.body, a_first).into_iter().map(Self::from_body).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Doc, OtError};
    use crate::sim::{run_fuzz, Rng, SimDoc};
    use serde_json::json;
    use std::collections::BTreeMap;

    /// 시험용 표: 행 id 목록 + (행 id, 열) -> 값. 행은 index로 넣고 지우고, 셀은 행 id로 가리킨다.
    struct Grid;
    impl CmdRules for Grid {
        fn transform(a: &CmdBody, b: &CmdBody, a_first: bool) -> Vec<CmdBody> {
            let n = |v: &Value, k: &str| v[k].as_u64().unwrap();
            let mk = |name: &str, args: Value| vec![CmdBody { name: name.into(), args }];
            match (a.name.as_str(), b.name.as_str()) {
                ("ins_row", "ins_row") => {
                    let (i, j) = (n(&a.args, "i"), n(&b.args, "i"));
                    mk("ins_row", json!({"i": if j < i || (j == i && !a_first) { i + 1 } else { i }, "id": a.args["id"]}))
                }
                ("ins_row", "del_row") => {
                    let (i, j) = (n(&a.args, "i"), n(&b.args, "i"));
                    mk("ins_row", json!({"i": if j < i { i - 1 } else { i }, "id": a.args["id"]}))
                }
                ("del_row", "ins_row") => {
                    let (i, j) = (n(&a.args, "i"), n(&b.args, "i"));
                    mk("del_row", json!({"i": if j <= i { i + 1 } else { i }, "id": a.args["id"]}))
                }
                ("del_row", "del_row") => {
                    let (i, j) = (n(&a.args, "i"), n(&b.args, "i"));
                    if i == j { vec![] } else { mk("del_row", json!({"i": if j < i { i - 1 } else { i }, "id": a.args["id"]})) }
                }
                // 셀 쓰기: 같은 셀은 늦은 쪽이 이기고, 행이 지워졌으면 버린다
                ("set_cell", "set_cell") if a.args["row"] == b.args["row"] && a.args["col"] == b.args["col"] && a_first => vec![],
                ("set_cell", "del_row") if a.args["row"] == b.args["id"] => vec![],
                _ => vec![a.clone()],
            }
        }
    }

    #[derive(Clone, Debug)]
    struct GridDoc {
        rows: Vec<u64>,
        cells: BTreeMap<(u64, u64), i64>,
        next: u64,
    }
    impl Doc for GridDoc {
        type Op = CmdOp<Grid>;
        fn apply(&mut self, op: &Self::Op) -> Result<(), OtError> {
            let a = &op.body.args;
            let bad = || Err(OtError::Invalid(format!("{op:?}")));
            match op.body.name.as_str() {
                "ins_row" => {
                    let i = a["i"].as_u64().unwrap() as usize;
                    if i > self.rows.len() {
                        return bad();
                    }
                    self.rows.insert(i, a["id"].as_u64().unwrap());
                }
                "del_row" => {
                    let i = a["i"].as_u64().unwrap() as usize;
                    if self.rows.get(i).copied() != a["id"].as_u64() {
                        return bad();
                    }
                    let id = self.rows.remove(i);
                    self.cells.retain(|(r, _), _| *r != id);
                }
                "set_cell" => {
                    let r = a["row"].as_u64().unwrap();
                    if !self.rows.contains(&r) {
                        return bad();
                    }
                    self.cells.insert((r, a["col"].as_u64().unwrap()), a["v"].as_i64().unwrap());
                }
                _ => return bad(),
            }
            Ok(())
        }
        fn canon(&self) -> Value {
            json!({"rows": self.rows, "cells": self.cells.iter().map(|((r, c), v)| json!([r, c, v])).collect::<Vec<_>>()})
        }
    }
    impl SimDoc for GridDoc {
        fn initial() -> Self {
            GridDoc { rows: vec![1, 2], cells: BTreeMap::new(), next: 1000 }
        }
        fn random_op(&self, rng: &mut Rng) -> Option<Self::Op> {
            // id는 클라이언트마다 겹치지 않게 난수 큰 값(실제는 id 임대)
            let fresh = 1_000_000 + rng.next() % 1_000_000_000;
            Some(match rng.below(4) {
                0 => CmdOp::new("ins_row", json!({"i": rng.below(self.rows.len() + 1), "id": fresh})),
                1 if !self.rows.is_empty() => {
                    let i = rng.below(self.rows.len());
                    CmdOp::new("del_row", json!({"i": i, "id": self.rows[i]}))
                }
                _ if !self.rows.is_empty() => CmdOp::new("set_cell", json!({"row": self.rows[rng.below(self.rows.len())], "col": rng.below(3), "v": rng.below(100)})),
                _ => return None,
            })
        }
    }

    #[test]
    fn cmd_rules_converge() {
        let _ = GridDoc::initial().next;
        if let Err(f) = run_fuzz::<GridDoc>(0..300, 3, 30) {
            panic!("{f:?}");
        }
    }

    #[test]
    fn cmd_wire_shape() {
        let op: CmdOp = CmdOp::new("fill", json!({"r": 1}));
        let s = serde_json::to_string(&op).unwrap();
        assert_eq!(s, r#"{"t":"cmd","c":"fill","a":{"r":1}}"#);
        let back: CmdOp = serde_json::from_str(&s).unwrap();
        assert_eq!(back, op);
    }
}
