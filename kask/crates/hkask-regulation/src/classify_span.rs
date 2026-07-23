//! Classification Regulation spans.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassifySpan {
    ClassifyDualFidelity,
    ClassifyDrift,
}

impl ClassifySpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClassifySpan::ClassifyDualFidelity => "reg.classify.dual_fidelity",
            ClassifySpan::ClassifyDrift => "reg.classify.drift",
        }
    }
}

impl std::fmt::Display for ClassifySpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ClassifySpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.classify.dual_fidelity" => Ok(ClassifySpan::ClassifyDualFidelity),
            "reg.classify.drift" => Ok(ClassifySpan::ClassifyDrift),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for ClassifySpan {
    fn as_str(&self) -> &'static str {
        ClassifySpan::as_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn classify_span_namespaces_are_canonical() {
        let all = vec![
            ClassifySpan::ClassifyDualFidelity,
            ClassifySpan::ClassifyDrift,
        ];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "ClassifySpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }
}
