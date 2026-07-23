//! ACP (Agent Communication Protocol) Regulation spans.
use hkask_types::ObservableSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpSpan {
    AcpUserPodMemorySize,
    AcpIdeConnectionState,
}

impl AcpSpan {
    pub fn as_str(&self) -> &'static str {
        match self {
            AcpSpan::AcpUserPodMemorySize => "reg.acp.userpod.memory_size",
            AcpSpan::AcpIdeConnectionState => "reg.acp.ide.connection_state",
        }
    }
}

impl std::fmt::Display for AcpSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for AcpSpan {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.acp.userpod.memory_size" => Ok(AcpSpan::AcpUserPodMemorySize),
            "reg.acp.ide.connection_state" => Ok(AcpSpan::AcpIdeConnectionState),
            _ => Err(()),
        }
    }
}

impl ObservableSpan for AcpSpan {
    fn as_str(&self) -> &'static str {
        AcpSpan::as_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn acp_span_namespaces_are_canonical() {
        let all = vec![
            AcpSpan::AcpUserPodMemorySize,
            AcpSpan::AcpIdeConnectionState,
        ];
        for span in all {
            let ns = SpanNamespace::new(span.as_str()).unwrap();
            assert_eq!(
                ns.as_str(),
                span.as_str(),
                "AcpSpan::as_str() must match CANONICAL_NAMESPACES"
            );
        }
    }
}
