use crate::db::KaskSkillId;
use sea_orm::entity::prelude::*;
use time::PrimitiveDateTime;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "kask_skill_versions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub kask_skill_id: KaskSkillId,
    #[sea_orm(primary_key)]
    pub version: String,
    pub published_at: PrimitiveDateTime,
    pub dependencies: String,
    pub tarball_sha256: String,
    pub download_count: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::kask_skill::Entity",
        from = "Column::KaskSkillId",
        to = "super::kask_skill::Column::Id"
    )]
    KaskSkill,
}

impl Related<super::kask_skill::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::KaskSkill.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
