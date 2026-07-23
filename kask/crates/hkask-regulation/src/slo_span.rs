//! SLO evaluation Regulation spans.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SloSpan {
    SloEvaluated,
}

impl SloSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            SloSpan::SloEvaluated => "reg.slo.evaluated",
        }
    }
}

impl std::fmt::Display for SloSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SloSpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.slo.evaluated" => Ok(SloSpan::SloEvaluated),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for SloSpan {
    fn as_str(&self) -> &'static str {
        SloSpan::as_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn slo_span_namespaces_are_canonical() {
        let all = vec![SloSpan::SloEvaluated];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "SloSpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }
}
