//! 레지스터 op: 키 -> JSON 값, 서버 순서의 마지막 쓰기가 이긴다. `value`가 None이면 키 삭제.
//! 도형 속성·셀 값처럼 통째로 덮어써도 되는 것에 쓴다(변환 불필요, 같은 키만 충돌).
use crate::core::{Doc, Invert, OtError, Transform};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename = "reg")]
pub struct RegOp {
    pub key: String,
    /// None이면 삭제
    #[serde(rename = "v", default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

impl RegOp {
    pub fn set(key: impl Into<String>, value: Value) -> Self {
        RegOp { key: key.into(), value: Some(value) }
    }
    pub fn del(key: impl Into<String>) -> Self {
        RegOp { key: key.into(), value: None }
    }
}

impl Transform for RegOp {
    fn transform(&self, other: &Self, a_first: bool) -> Vec<Self> {
        // 같은 키: 서버 순서가 늦은 쪽이 이긴다. other가 이미 적용됐으므로 self가 앞서면(=other가 늦으면) 버린다.
        if self.key == other.key && a_first {
            return vec![];
        }
        vec![self.clone()]
    }
}

/// 키-값 문서.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegDoc(pub BTreeMap<String, Value>);

impl Doc for RegDoc {
    type Op = RegOp;
    fn apply(&mut self, op: &RegOp) -> Result<(), OtError> {
        match &op.value {
            Some(v) => {
                self.0.insert(op.key.clone(), v.clone());
            }
            None => {
                self.0.remove(&op.key);
            }
        }
        Ok(())
    }
    fn canon(&self) -> Value {
        Value::Object(self.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

impl Invert<RegDoc> for RegOp {
    fn invert(&self, before: &RegDoc) -> Vec<Self> {
        vec![RegOp { key: self.key.clone(), value: before.0.get(&self.key).cloned() }]
    }
}
