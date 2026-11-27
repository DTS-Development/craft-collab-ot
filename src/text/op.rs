//! 한 문서 안 여러 글(본문·셀·도형 글·각주…)을 key로 구분하는 op와 시험용 문서.
//!
//! wire: `{"t":"text","key":"...","d":[...Delta op들...]}`.

use super::delta::{Delta, Op};
use crate::core::{Doc, Invert, OtError, Transform};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct TextOp {
    pub key: String,
    pub delta: Delta,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    t: String,
    key: String,
    d: Vec<Op>,
}

impl Serialize for TextOp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Wire {
            t: "text".into(),
            key: self.key.clone(),
            d: self.delta.ops().to_vec(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TextOp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = Wire::deserialize(d)?;
        if w.t != "text" {
            return Err(serde::de::Error::custom("text op가 아니다"));
        }
        Ok(TextOp {
            key: w.key,
            delta: Delta::from_ops(w.d),
        })
    }
}

impl Transform for TextOp {
    /// `self`를 `other`가 이미 적용된 상태에 맞춘다. 키가 다르면 그대로.
    /// Delta::transform(x, y, p)는 y를 x 뒤로 옮기므로 방향을 뒤집어 부른다.
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self> {
        if self.key != other.key {
            return vec![self.clone()];
        }
        let delta = other.delta.transform(&self.delta, !a_first);
        if delta.is_empty() {
            Vec::new()
        } else {
            vec![TextOp {
                key: self.key.clone(),
                delta,
            }]
        }
    }
}

/// 퍼즈·시험용 문서: key -> 글 Delta(삽입만 있는 문서).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextDoc(pub BTreeMap<String, Delta>);

impl Doc for TextDoc {
    type Op = TextOp;

    fn apply(&mut self, op: &TextOp) -> Result<(), OtError> {
        let base = self.0.entry(op.key.clone()).or_default();
        base.check_on(&op.delta).map_err(OtError::Invalid)?;
        *base = base.compose(&op.delta);
        Ok(())
    }

    fn canon(&self) -> Value {
        let m: serde_json::Map<String, Value> = self
            .0
            .iter()
            .filter(|(_, d)| !d.is_empty())
            .map(|(k, d)| (k.clone(), json!(d.ops())))
            .collect();
        Value::Object(m)
    }
}

impl Invert<TextDoc> for TextOp {
    fn invert(&self, before: &TextDoc) -> Vec<Self> {
        let empty = Delta::new();
        let base = before.0.get(&self.key).unwrap_or(&empty);
        vec![TextOp {
            key: self.key.clone(),
            delta: self.delta.invert(base),
        }]
    }
}
