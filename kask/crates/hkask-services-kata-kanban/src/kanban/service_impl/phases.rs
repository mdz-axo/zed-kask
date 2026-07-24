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

    pub fn task_set_phase(&self, task_id: TaskId, phase_id: PhaseId) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        task.phase_id = Some(phase_id);
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }

    pub fn tasks_by_phase(
        &self,
        board_id: BoardId,
        phase_id: PhaseId,
    ) -> Result<Vec<Task>, KanbanError> {
        let all = self.task_list(board_id, TaskFilter::all())?;
        Ok(all
            .into_iter()
            .filter(|t| t.phase_id == Some(phase_id))
            .collect())
    }
}
