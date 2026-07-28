use crate::db::KaskSkillId;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "kask_skills")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: KaskSkillId,
    pub source_user: String,
    pub skill_name: String,
    pub description: String,
    pub latest_version: String,
    pub total_download_count: i64,
    pub upvote_count: i64,
    pub downvote_count: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::kask_skill_version::Entity")]
    Versions,
    #[sea_orm(has_many = "super::kask_skill_vote::Entity")]
    Votes,
}

impl Related<super::kask_skill_version::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Versions.def()
    }
}

impl Related<super::kask_skill_vote::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Votes.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
