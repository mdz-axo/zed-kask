//! QA repair Regulation spans.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QaSpan {
    QaRepairAttempted,
    QaRepairVerified,
    QaRepairExhausted,
}

impl QaSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            QaSpan::QaRepairAttempted => "reg.qa.repair_attempted",
            QaSpan::QaRepairVerified => "reg.qa.repair_verified",
            QaSpan::QaRepairExhausted => "reg.qa.repair_exhausted",
        }
    }
}

impl std::fmt::Display for QaSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for QaSpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.qa.repair_attempted" => Ok(QaSpan::QaRepairAttempted),
            "reg.qa.repair_verified" => Ok(QaSpan::QaRepairVerified),
            "reg.qa.repair_exhausted" => Ok(QaSpan::QaRepairExhausted),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for QaSpan {
    fn as_str(&self) -> &'static str {
        QaSpan::as_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn qa_span_namespaces_are_canonical() {
        let all = vec![
            QaSpan::QaRepairAttempted,
            QaSpan::QaRepairVerified,
            QaSpan::QaRepairExhausted,
        ];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "QaSpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }
}
