//! Core ID system — generic UUID-based identifiers with phantom type parameters.
//!
use std::fmt::Debug;
use std::marker::PhantomData;
use uuid::Uuid;

mod private {
    pub trait Sealed {}
}

/// Marker trait for ID kind — enables type-safe ID types via phantom generics.
/// The `Sealed` supertrait prevents external implementations.
pub trait IdKind: private::Sealed + 'static {}

/// Generic UUID-based identifier with phantom type parameter.
///
/// `Id<BotKind>` and `Id<TemplateKind>` are different types — you can't
/// accidentally pass a `BotID` where a `TemplateID` is expected. All common
/// functionality (construction, parsing, display, hashing) lives here once.
pub struct Id<T: IdKind> {
    uuid: Uuid,
    _marker: PhantomData<T>,
}

// ── Manual trait impls (avoid derived bounds on phantom type parameter T) ──

impl<T: IdKind> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: IdKind> Copy for Id<T> {}

impl<T: IdKind> Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Id").field(&self.uuid).finish()
    }
}

impl<T: IdKind> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<T: IdKind> Eq for Id<T> {}

impl<T: IdKind> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<T: IdKind> serde::Serialize for Id<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.uuid.serialize(serializer)
    }
}

impl<'de, T: IdKind> serde::Deserialize<'de> for Id<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Uuid::deserialize(deserializer).map(Id::from_uuid)
    }
}

impl<T: IdKind> Id<T> {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  (no inputs)
    /// post: returns a unique [`Id<T>`] wrapping a random UUID v4
    pub fn new() -> Self {
        Self {
            uuid: Uuid::new_v4(),
            _marker: PhantomData,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  uuid is any valid [`Uuid`]
    /// post: returns an [`Id<T>`] wrapping the given uuid unchanged
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            _marker: PhantomData,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid [`Id<T>`]
    /// post: returns the inner [`Uuid`] unchanged
    pub fn as_uuid(&self) -> Uuid {
        self.uuid
    }
}

impl<T: IdKind> std::str::FromStr for Id<T> {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self::from_uuid)
    }
}

impl<T: IdKind> Default for Id<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: IdKind> std::fmt::Display for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

// ── Kind types (sealed, empty enums for phantom type parameters) ─────────

pub enum TemplateKind {}
impl private::Sealed for TemplateKind {}
impl IdKind for TemplateKind {}

pub enum BotKind {}
impl private::Sealed for BotKind {}
impl IdKind for BotKind {}

pub enum TripleKind {}
impl private::Sealed for TripleKind {}
impl IdKind for TripleKind {}

pub enum EventKind {}
impl private::Sealed for EventKind {}
impl IdKind for EventKind {}

pub enum GoalKind {}
impl private::Sealed for GoalKind {}
impl IdKind for GoalKind {}

pub enum EmbeddingKind {}
impl private::Sealed for EmbeddingKind {}
impl IdKind for EmbeddingKind {}

pub enum UserKind {}
impl private::Sealed for UserKind {}
impl IdKind for UserKind {}

pub enum EscalationKind {}
impl private::Sealed for EscalationKind {}
impl IdKind for EscalationKind {}

pub enum PhaseKind {}
impl private::Sealed for PhaseKind {}
impl IdKind for PhaseKind {}

pub enum CommentKind {}
impl private::Sealed for CommentKind {}
impl IdKind for CommentKind {}

pub enum BoardKind {}
impl private::Sealed for BoardKind {}
impl IdKind for BoardKind {}

pub enum ColumnKind {}
impl private::Sealed for ColumnKind {}
impl IdKind for ColumnKind {}

pub enum TaskKind {}
impl private::Sealed for TaskKind {}
impl IdKind for TaskKind {}

// ── Type aliases ──────────────────────────────────────────────────────────

pub type TemplateID = Id<TemplateKind>;
pub type BotID = Id<BotKind>;
pub type HMemId = Id<TripleKind>;
pub type EventID = Id<EventKind>;
pub(crate) type GoalID = Id<GoalKind>;
pub type EmbeddingID = Id<EmbeddingKind>;
pub(crate) type UserID = Id<UserKind>;

pub type EscalationID = Id<EscalationKind>;
pub type PhaseId = Id<PhaseKind>;
pub type CommentId = Id<CommentKind>;
pub type BoardId = Id<BoardKind>;
pub type ColumnId = Id<ColumnKind>;
pub type TaskId = Id<TaskKind>;
