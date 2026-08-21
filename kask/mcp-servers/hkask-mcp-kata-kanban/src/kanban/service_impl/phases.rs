use super::*;

impl KanbanService {
    pub fn board_add_phase(
        &self,
        board_id: BoardId,
        name: &str,
        order: u32,
    ) -> Result<KanbanPhase, KanbanError> {
        let mut board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        let phase = KanbanPhase::new(name.to_string(), order);
        board.phases.push(phase.clone());
        self.update_board_triple(&board)?;
        Ok(phase)
    }
}
